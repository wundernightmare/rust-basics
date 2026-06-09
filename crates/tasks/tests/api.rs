//! Full-vertical integration test for the tasks service — HTTP → Valkey → Kafka
//! — driven against the assembled router (no socket) with real containers. It
//! no-ops (prints a skip line) when Docker is unavailable.

use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::GenericImage;
use testcontainers_modules::kafka::apache::{Kafka as KafkaImage, KAFKA_PORT};
use tokio::sync::Mutex;
use tower::ServiceExt;

use tasks::{routes, AppState, TaskStore};

const TOPIC: &str = "tasks.events.it";

struct Stack {
    _valkey: testcontainers::ContainerAsync<GenericImage>,
    _kafka: testcontainers::ContainerAsync<KafkaImage>,
    valkey_url: String,
    brokers: String,
}

async fn bring_up() -> Option<Stack> {
    let valkey = GenericImage::new("valkey/valkey", "9.0")
        .with_wait_for(WaitFor::message_on_stdout("Ready to accept connections"))
        .with_exposed_port(6379.tcp())
        .start()
        .await;
    let valkey = match valkey {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipping tasks integration test (docker unavailable?): {e}");
            return None;
        }
    };
    let vport = valkey.get_host_port_ipv4(6379).await.ok()?;

    let kafka = KafkaImage::default().start().await.ok()?;
    let kport = kafka.get_host_port_ipv4(KAFKA_PORT).await.ok()?;

    Some(Stack {
        _valkey: valkey,
        _kafka: kafka,
        valkey_url: format!("redis://127.0.0.1:{vport}"),
        brokers: format!("127.0.0.1:{kport}"),
    })
}

fn kafka_cfg(brokers: &str, group: &str) -> kafka::Config {
    kafka::Config {
        kafka_brokers: brokers.to_owned(),
        kafka_topic: TOPIC.to_owned(),
        kafka_group: group.to_owned(),
        kafka_client_id: "tasks-it".to_owned(),
        kafka_probe_secs: 1,
    }
}

async fn post(router: &axum::Router, uri: &str, body: &str) -> (StatusCode, String, Value) {
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    read(resp).await
}

async fn request(router: &axum::Router, method: &str, uri: &str) -> (StatusCode, String, Value) {
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    read(resp).await
}

async fn read(resp: axum::response::Response) -> (StatusCode, String, Value) {
    let status = resp.status();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, content_type, json)
}

#[tokio::test]
async fn tasks_end_to_end() {
    let Some(stack) = bring_up().await else {
        return;
    };

    let cache = valkey::Cache::connect(&valkey::Config {
        valkey_url: stack.valkey_url.clone(),
        valkey_probe_secs: 1,
    })
    .await
    .expect("valkey");
    let producer = kafka::Producer::connect(&kafka_cfg(&stack.brokers, "tasks-producer"))
        .await
        .expect("producer");

    let router = routes(AppState {
        store: TaskStore::new(cache),
        producer,
        topic: TOPIC.to_owned(),
    });

    // create
    let (status, _ct, created) = post(&router, "/tasks", r#"{"title":"ship it"}"#).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["title"], "ship it");
    let id = created["id"].as_str().unwrap().to_owned();

    // read
    let (status, _ct, got) = request(&router, "GET", &format!("/tasks/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got["id"], id);

    // list
    let (status, _ct, list) = request(&router, "GET", "/tasks").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["tasks"].as_array().unwrap().len(), 1);

    // empty title → 400 problem+json
    let (status, ct, problem) = post(&router, "/tasks", r#"{"title":""}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(ct.contains("application/problem+json"));
    assert_eq!(problem["code"], "empty_title");

    // the task.created event reached Kafka
    require_event_delivered(&stack.brokers, &id).await;

    // delete, then 404
    let (status, _ct, _b) = request(&router, "DELETE", &format!("/tasks/{id}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, ct, problem) = request(&router, "GET", &format!("/tasks/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(ct.contains("application/problem+json"));
    assert_eq!(problem["code"], "task_not_found");
}

async fn require_event_delivered(brokers: &str, want_id: &str) {
    let consumer = kafka::Consumer::connect(&kafka_cfg(brokers, "tasks-it-verify"))
        .await
        .expect("consumer");
    let seen = Arc::new(Mutex::new(false));

    let found = Arc::clone(&seen);
    let want = want_id.to_owned();
    let run = tokio::spawn(async move {
        let _ = consumer
            .run(move |msg| {
                let found = Arc::clone(&found);
                let want = want.clone();
                async move {
                    if let Ok(evt) = serde_json::from_slice::<Value>(&msg.payload) {
                        if evt["id"] == want {
                            *found.lock().await = true;
                        }
                    }
                    Ok(())
                }
            })
            .await;
    });

    let delivered = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if *seen.lock().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .is_ok();
    run.abort();
    assert!(delivered, "task.created event was not delivered to Kafka");
}
