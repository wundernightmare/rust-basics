//! Integration test: produce events to a real Kafka broker, run the worker, and
//! assert it counts consumed vs. skipped. No-ops when Docker is unavailable.

use std::time::Duration;

use prometheus::Registry;
use testcontainers_modules::kafka::apache::{Kafka as KafkaImage, KAFKA_PORT};
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::ContainerAsync;

use consumer::Worker;

const TOPIC: &str = "tasks.events.consumer-it";

fn cfg(brokers: &str, group: &str) -> kafka::Config {
    kafka::Config {
        kafka_brokers: brokers.to_owned(),
        kafka_topic: TOPIC.to_owned(),
        kafka_group: group.to_owned(),
        kafka_client_id: "consumer-it".to_owned(),
        kafka_probe_secs: 1,
    }
}

async fn start_broker() -> Option<(ContainerAsync<KafkaImage>, String)> {
    let node = match KafkaImage::default().start().await {
        Ok(n) => n,
        Err(e) => {
            eprintln!("skipping consumer integration test (docker unavailable?): {e}");
            return None;
        }
    };
    let port = node.get_host_port_ipv4(KAFKA_PORT).await.ok()?;
    Some((node, format!("127.0.0.1:{port}")))
}

fn counter(registry: &Registry, name: &str) -> f64 {
    for mf in registry.gather() {
        if mf.name() == name {
            return mf
                .get_metric()
                .first()
                .map_or(0.0, |m| m.get_counter().value());
        }
    }
    0.0
}

#[tokio::test]
async fn consumes_valid_and_skips_garbage() {
    let Some((_node, brokers)) = start_broker().await else {
        return;
    };

    let producer = kafka::Producer::connect(&cfg(&brokers, "producer"))
        .await
        .expect("producer");
    for id in ["a", "b"] {
        let payload = serde_json::to_vec(&serde_json::json!({ "id": id, "title": "t" })).unwrap();
        producer.publish("", id.as_bytes(), &payload).await.unwrap();
    }
    // a poison record that the worker should skip, not choke on
    producer.publish("", b"bad", b"not json").await.unwrap();

    let kafka_consumer = kafka::Consumer::connect(&cfg(&brokers, "consumer-it-group"))
        .await
        .expect("consumer");
    let registry = Registry::new();
    let worker = Worker::new(kafka_consumer, &registry).expect("worker");
    assert!(worker.readiness_check()().is_ok());

    let run = tokio::spawn(worker.run());

    let done = tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            if counter(&registry, "consumer_tasks_consumed_total") >= 2.0
                && counter(&registry, "consumer_tasks_skipped_total") >= 1.0
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .is_ok();
    run.abort();

    assert!(done, "worker should consume 2 and skip 1");
    assert!((counter(&registry, "consumer_tasks_consumed_total") - 2.0).abs() < f64::EPSILON);
    assert!((counter(&registry, "consumer_tasks_skipped_total") - 1.0).abs() < f64::EPSILON);
}
