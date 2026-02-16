use crate::MAC_ADDRESS;
use anyhow::Result;
use async_channel::Receiver;
use esp_idf_svc::{
    mqtt::client::{EspAsyncMqttClient, EspAsyncMqttConnection, MqttClientConfiguration, QoS},
    sys::EspError,
    timer::EspAsyncTimer,
};
use log::{error, info, warn};
use std::{pin, time::Duration};
use tokio::{io::join, select};

// const MQTT_URL: &str = "mqtt://192.168.0.87:1883";
const MQTT_CLIENT_ID: &str = MAC_ADDRESS;
const MQTT_USERNAME: Option<&str> = option_env!("MQTT_USERNAME");
const MQTT_PASSWORD: Option<&str> = option_env!("MQTT_PASSWORD");

const MQTT_TOPIC_BLCA: &str = "ippras.ru/blca/#";
const MQTT_TOPIC_TEMPERATURE: &str = "ippras.ru/blca/temperature";

const RETRY: Duration = Duration::from_millis(500);
const SLEEP: Duration = Duration::from_secs(1);

const MQTT_URL: &str = "mqtt://broker.emqx.io:1883";
const MQTT_TOPIC: &str = MQTT_TOPIC_TEMPERATURE;

pub(crate) fn initialize() -> Result<(EspAsyncMqttClient, EspAsyncMqttConnection), EspError> {
    info!("initialize mqtt");
    Ok(EspAsyncMqttClient::new(
        MQTT_URL,
        &MqttClientConfiguration {
            client_id: Some(MQTT_CLIENT_ID),
            username: MQTT_USERNAME,
            password: MQTT_PASSWORD,
            ..Default::default()
        },
    )?)
}

pub(crate) async fn run(
    client: &mut EspAsyncMqttClient,
    connection: &mut EspAsyncMqttConnection,
    timer: &mut EspAsyncTimer,
) -> Result<(), EspError> {
    info!("About to start the MQTT client");

    select!(
        // Need to immediately start pumping the connection for messages, or else subscribe() and publish() below will not work
        // Note that when using the alternative structure and the alternative constructor - `EspMqttClient::new_cb` - you don't need to
        // spawn a new thread, as the messages will be pumped with a backpressure into the callback you provide.
        // Yet, you still need to efficiently process each message in the callback without blocking for too long.
        //
        // Note also that if you go to http://tools.emqx.io/ and then connect and send a message to topic
        // "esp-mqtt-demo", the client configured here should receive it.
        _ = async move {
            info!("MQTT Listening for messages");

            while let Ok(event) = connection.next().await {
                info!("[Queue] Event: {}", event.payload());
            }

            info!("Connection closed");

            // Ok(())
        } => {},
        _ = async move {
            // Using `pin!` is optional, but it optimizes the memory size of the Futures
            loop {
                if let Err(e) = client.subscribe(MQTT_TOPIC, QoS::AtMostOnce).await {
                    error!("Failed to subscribe to topic \"{MQTT_TOPIC}\": {e}, retrying...");

                    // Re-try in 0.5s
                    timer.after(Duration::from_millis(500)).await.unwrap();

                    continue;
                }

                info!("Subscribed to topic \"{MQTT_TOPIC}\"");

                // Just to give a chance of our connection to get even the first published message
                timer.after(Duration::from_millis(500)).await.unwrap();

                let payload = "Hello from esp-mqtt-demo!";

                loop {
                    client
                        .publish(MQTT_TOPIC, QoS::AtMostOnce, false, payload.as_bytes())
                        .await.unwrap();

                    info!("Published \"{payload}\" to topic \"{MQTT_TOPIC}\"");

                    let sleep_secs = 2;

                    info!("Now sleeping for {sleep_secs}s...");
                    timer.after(Duration::from_secs(sleep_secs)).await.unwrap();
                }
            }
        } => {},
    );
    Ok(())
    // join(reader, writer)
    // match res {
    //     Either::First(res) => res,
    //     Either::Second(res) => res,
    // }
}

// Subscriber
pub(crate) async fn subscriber(mut connection: EspAsyncMqttConnection) {
    while let Ok(event) = connection.next().await {
        info!("Subscribed: {}", event.payload());
    }
    warn!("MQTT connection closed");
}

// Publisher
pub(crate) async fn publisher(
    mut client: EspAsyncMqttClient,
    mut timer: EspAsyncTimer,
) -> Result<()> {
    loop {
        if let Err(error) = client.subscribe(MQTT_TOPIC_BLCA, QoS::ExactlyOnce).await {
            warn!(r#"Retry to subscribe to topic "{MQTT_TOPIC_BLCA}": {error}"#);
            timer.after(RETRY).await?;
            continue;
        }
        info!(r#"Subscribed to topic "{MQTT_TOPIC_BLCA}""#);
        // Just to give a chance of our connection to get even the first published message
        timer.after(SLEEP).await?;
        loop {
            // let serialized = ron::to_string(temperature)?;
            client
                .publish(
                    MQTT_TOPIC_TEMPERATURE,
                    QoS::ExactlyOnce,
                    false,
                    b"serialized.as_bytes()",
                )
                .await?;
            // info!(r#"Published "{serialized}" to topic "{MQTT_TOPIC_TEMPERATURE}""#);
        }
    }
}
