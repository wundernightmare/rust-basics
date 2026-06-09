//! Integration tests for the Kafka producer/consumer against a real broker
//! (Apache Kafka, `KRaft`) via testcontainers. They no-op (print a skip line)
//! when Docker is unavailable.

use std::sync::Arc;
use std::time::Duration;

use testcontainers_modules::kafka::apache::{Kafka as KafkaImage, KAFKA_PORT};
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::ContainerAsync;
use tokio::sync::Mutex;

use kafka::{Config, Consumer, Producer};

async fn start_broker() -> Option<(ContainerAsync<KafkaImage>, Config)> {
    let node = match KafkaImage::default().start().await {
        Ok(n) => n,
        Err(e) => {
            eprintln!("skipping kafka integration test (docker unavailable?): {e}");
            return None;
        }
    };
    let port = node
        .get_host_port_ipv4(KAFKA_PORT)
        .await
        .expect("mapped port");
    let cfg = Config {
        kafka_brokers: format!("127.0.0.1:{port}"),
        kafka_topic: "it.events".to_owned(),
        kafka_group: "it-group".to_owned(),
        kafka_client_id: "it".to_owned(),
        kafka_probe_secs: 1,
    };
    Some((node, cfg))
}

#[tokio::test]
async fn produce_consume_round_trip() {
    let Some((_node, cfg)) = start_broker().await else {
        return;
    };

    let producer = Producer::connect(&cfg).await.expect("producer");
    assert!(producer.readiness_check()().is_ok());

    for i in 0..5 {
        producer
            .publish(
                "it.events",
                format!("k{i}").as_bytes(),
                format!("v{i}").as_bytes(),
            )
            .await
            .expect("publish");
    }

    let consumer = Consumer::connect(&cfg).await.expect("consumer");
    let got: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let collected = Arc::clone(&got);
    let run = tokio::spawn(async move {
        let _ = consumer
            .run(move |msg| {
                let collected = Arc::clone(&collected);
                async move {
                    collected
                        .lock()
                        .await
                        .push(String::from_utf8_lossy(&msg.payload).into_owned());
                    Ok(())
                }
            })
            .await;
    });

    let all = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if got.lock().await.len() >= 5 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .is_ok();
    run.abort();

    assert!(all, "consumer should drain all 5 records");
    let mut values = got.lock().await.clone();
    values.sort();
    assert_eq!(values, vec!["v0", "v1", "v2", "v3", "v4"]);
}
