use crate::{MAC_ADDRESS, temperature::Request as TemperatureRequest};
use anyhow::Result;
use esp_idf_svc::{
    mqtt::client::{EspAsyncMqttClient, EspAsyncMqttConnection, MqttClientConfiguration, QoS},
    sys::EspError,
};
use log::{error, info, trace, warn};
use tokio::{
    spawn,
    sync::{mpsc::Sender, oneshot},
    time::{Duration, sleep},
};

const MQTT_URL: &str = "mqtt://192.168.0.87:1883";
const MQTT_CLIENT_ID: &str = MAC_ADDRESS;
const MQTT_USERNAME: Option<&str> = option_env!("MQTT_USERNAME");
const MQTT_PASSWORD: Option<&str> = option_env!("MQTT_PASSWORD");

const MQTT_TOPIC_BLC: &str = "ippras.ru/blca/#";
const MQTT_TOPIC_TEMPERATURE: &str = "ippras.ru/blca/temperature";

const RETRY: Duration = Duration::from_millis(500);

pub(crate) async fn run(
    mut temperature_sender: Sender<TemperatureRequest>,
) -> Result<(), EspError> {
    info!("Initialize MQTT");
    let (mut client, connection) = EspAsyncMqttClient::new(
        MQTT_URL,
        &MqttClientConfiguration {
            client_id: Some(MQTT_CLIENT_ID),
            username: MQTT_USERNAME,
            password: MQTT_PASSWORD,
            ..Default::default()
        },
    )?;
    spawn(subscriber(connection));
    loop {
        if let Err(error) = client.subscribe(MQTT_TOPIC_BLC, QoS::ExactlyOnce).await {
            warn!(r#"Retry to subscribe to topic "{MQTT_TOPIC_BLC}": {error}"#);
            sleep(RETRY).await;
            continue;
        }
        info!(r#"Subscribed to topic "{MQTT_TOPIC_BLC}""#);
        // Just to give a chance of our connection to get even the first published message.
        sleep(Duration::from_secs(1)).await;
        loop {
            if let Err(error) = publisher(&mut client, &mut temperature_sender).await {
                error!("{error}");
            }
            sleep(Duration::from_secs(1)).await;
        }
    }
}

// Subscriber
pub(crate) async fn subscriber(mut connection: EspAsyncMqttConnection) {
    info!("MQTT subscriber");
    loop {
        match connection.next().await {
            Ok(event) => trace!("Subscribed: {}", event.payload()),
            Err(error) => {
                error!("{error}");
                warn!("MQTT connection closed");
            }
        }
    }
}

// Publisher
pub(crate) async fn publisher(
    client: &mut EspAsyncMqttClient,
    temperature_sender: &mut Sender<TemperatureRequest>,
) -> Result<()> {
    info!("MQTT publisher");
    loop {
        let (sender, receiver) = oneshot::channel();
        temperature_sender.send((0..2, sender)).await?;
        let temperatures = receiver.await??;
        let serialized = ron::to_string(&temperatures)?;
        if let Err(error) = client
            .publish(
                MQTT_TOPIC_TEMPERATURE,
                QoS::ExactlyOnce,
                false,
                serialized.as_bytes(),
            )
            .await
        {
            error!("MQTT publish {error:?}");
        }
        sleep(Duration::from_secs(1)).await;
    }
}
