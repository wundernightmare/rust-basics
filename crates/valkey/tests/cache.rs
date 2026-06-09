//! Integration tests for the Valkey cache against a real server via
//! testcontainers. They no-op (print a skip line) when Docker is unavailable.

use std::time::Duration;

use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::GenericImage;

use valkey::{Cache, Config};

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Widget {
    id: String,
    name: String,
}

async fn start_cache() -> Option<(testcontainers::ContainerAsync<GenericImage>, Cache)> {
    let container = GenericImage::new("valkey/valkey", "9.0")
        .with_wait_for(WaitFor::message_on_stdout("Ready to accept connections"))
        .with_exposed_port(6379.tcp())
        .start()
        .await;
    let container = match container {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipping valkey integration test (docker unavailable?): {e}");
            return None;
        }
    };
    let port = container
        .get_host_port_ipv4(6379)
        .await
        .expect("mapped port");
    let cfg = Config {
        valkey_url: format!("redis://127.0.0.1:{port}"),
        valkey_probe_secs: 1,
    };
    let cache = Cache::connect(&cfg).await.expect("connect to valkey");
    Some((container, cache))
}

#[tokio::test]
async fn get_set_del_round_trip() {
    let Some((_c, cache)) = start_cache().await else {
        return;
    };

    assert!(cache.get("missing").await.unwrap().is_none());

    cache.set("greeting", "pong", None).await.unwrap();
    assert_eq!(
        cache.get("greeting").await.unwrap().as_deref(),
        Some("pong")
    );

    cache.del("greeting").await.unwrap();
    assert!(cache.get("greeting").await.unwrap().is_none());

    // readiness reflects a live server
    assert!(cache.readiness_check()().is_ok());
}

#[tokio::test]
async fn ttl_expires_the_key() {
    let Some((_c, cache)) = start_cache().await else {
        return;
    };

    cache
        .set("ephemeral", "x", Some(Duration::from_secs(1)))
        .await
        .unwrap();
    assert_eq!(cache.get("ephemeral").await.unwrap().as_deref(), Some("x"));

    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        cache.get("ephemeral").await.unwrap().is_none(),
        "key should have expired"
    );
}

#[tokio::test]
async fn json_and_set_index() {
    let Some((_c, cache)) = start_cache().await else {
        return;
    };

    let w = Widget {
        id: "7".to_owned(),
        name: "gadget".to_owned(),
    };
    cache.set_json("widget:7", &w, None).await.unwrap();
    cache.set_add("widgets", "7").await.unwrap();

    let got: Option<Widget> = cache.get_json("widget:7").await.unwrap();
    assert_eq!(got, Some(w));
    assert_eq!(cache.set_members("widgets").await.unwrap(), vec!["7"]);

    cache.set_remove("widgets", "7").await.unwrap();
    assert!(cache.set_members("widgets").await.unwrap().is_empty());
}
