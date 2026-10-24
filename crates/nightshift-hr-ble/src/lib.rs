use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use btleplug::{
    api::{
        Central, Manager as _, Peripheral as _, ScanFilter, bleuuid::uuid_from_u16,
    },
    platform::{Adapter, Manager, Peripheral},
};
use futures_util::StreamExt;
use tokio::sync::mpsc;

pub const HEART_RATE_SERVICE_UUID: uuid::Uuid = uuid_from_u16(0x180D);
pub const HEART_RATE_MEASUREMENT_UUID: uuid::Uuid = uuid_from_u16(0x2A37);
const DEFAULT_SCAN_DURATION: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BleDeviceInfo {
    pub id: String,
    pub address: String,
    pub name: Option<String>,
}

impl BleDeviceInfo {
    pub fn matches_filter(&self, filter: &str) -> bool {
        let filter = filter.trim().to_ascii_lowercase();
        if filter.is_empty() {
            return true;
        }

        self.id.to_ascii_lowercase().contains(&filter)
            || self.address.to_ascii_lowercase().contains(&filter)
            || self
                .name
                .as_deref()
                .is_some_and(|name| name.to_ascii_lowercase().contains(&filter))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BleHeartRateEvent {
    Connected(BleDeviceInfo),
    Measurement(u16),
    Disconnected,
}

pub fn parse_heart_rate_measurement(value: &[u8]) -> Result<u16> {
    let flags = *value.first().context("heart-rate measurement is empty")?;
    let value_is_u16 = flags & 0x01 != 0;

    if value_is_u16 {
        let bytes = value
            .get(1..3)
            .context("16-bit heart-rate measurement is truncated")?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    } else {
        Ok(u16::from(
            *value
                .get(1)
                .context("8-bit heart-rate measurement is truncated")?,
        ))
    }
}

pub async fn first_adapter() -> Result<Adapter> {
    let manager = Manager::new().await.context("creating Bluetooth manager")?;
    manager
        .adapters()
        .await
        .context("listing Bluetooth adapters")?
        .into_iter()
        .next()
        .context("no Bluetooth adapter is available")
}

async fn scan_heart_rate_peripherals(adapter: &Adapter) -> Result<Vec<Peripheral>> {
    adapter
        .start_scan(ScanFilter {
            services: vec![HEART_RATE_SERVICE_UUID],
        })
        .await
        .context("starting BLE heart-rate scan")?;

    tokio::time::sleep(DEFAULT_SCAN_DURATION).await;
    adapter.stop_scan().await.context("stopping BLE scan")?;

    let peripherals = adapter
        .peripherals()
        .await
        .context("listing discovered BLE peripherals")?;
    let mut heart_rate_peripherals = Vec::new();

    for peripheral in peripherals {
        let properties = peripheral
            .properties()
            .await
            .context("reading BLE peripheral properties")?;
        let Some(properties) = properties else {
            continue;
        };
        if properties.services.contains(&HEART_RATE_SERVICE_UUID) {
            heart_rate_peripherals.push(peripheral);
        }
    }

    Ok(heart_rate_peripherals)
}

fn sort_and_dedup_devices(devices: &mut Vec<BleDeviceInfo>) {
    devices.sort_by(|left, right| {
        left.name
            .is_none()
            .cmp(&right.name.is_none())
            .then_with(|| {
                left.name
                    .as_deref()
                    .unwrap_or("")
                    .cmp(right.name.as_deref().unwrap_or(""))
            })
            .then_with(|| left.address.cmp(&right.address))
            .then_with(|| left.id.cmp(&right.id))
    });
    devices.dedup_by(|left, right| left.id == right.id);
}

pub async fn discover_devices(adapter: &Adapter) -> Result<Vec<BleDeviceInfo>> {
    let mut devices = Vec::new();
    for peripheral in scan_heart_rate_peripherals(adapter).await? {
        devices.push(device_info(&peripheral).await?);
    }
    sort_and_dedup_devices(&mut devices);
    Ok(devices)
}

pub async fn discover_device(
    adapter: &Adapter,
    device_filter: Option<&str>,
) -> Result<Peripheral> {
    for peripheral in scan_heart_rate_peripherals(adapter).await? {
        if let Some(filter) = device_filter {
            let info = device_info(&peripheral).await?;
            if !info.matches_filter(filter) {
                continue;
            }
        }

        return Ok(peripheral);
    }

    match device_filter {
        Some(filter) => bail!("no BLE heart-rate device matched {filter:?}"),
        None => bail!("no BLE device advertising the Heart Rate service was found"),
    }
}

pub async fn device_info(peripheral: &Peripheral) -> Result<BleDeviceInfo> {
    let properties = peripheral
        .properties()
        .await
        .context("reading BLE heart-rate device properties")?;
    Ok(BleDeviceInfo {
        id: format!("{:?}", peripheral.id()),
        address: peripheral.address().to_string(),
        name: properties.and_then(|properties| properties.local_name),
    })
}

pub async fn run_device(
    peripheral: Peripheral,
    events: mpsc::Sender<BleHeartRateEvent>,
) -> Result<()> {
    peripheral
        .connect()
        .await
        .context("connecting BLE heart-rate device")?;
    peripheral
        .discover_services()
        .await
        .context("discovering BLE heart-rate services")?;

    let characteristic = peripheral
        .characteristics()
        .into_iter()
        .find(|characteristic| characteristic.uuid == HEART_RATE_MEASUREMENT_UUID)
        .ok_or_else(|| anyhow!("heart-rate device does not expose measurement characteristic 0x2A37"))?;

    let info = device_info(&peripheral).await?;
    events
        .send(BleHeartRateEvent::Connected(info))
        .await
        .map_err(|_| anyhow!("heart-rate event consumer stopped"))?;

    let mut notifications = peripheral
        .notifications()
        .await
        .context("opening BLE notification stream")?;
    peripheral
        .subscribe(&characteristic)
        .await
        .context("subscribing to heart-rate measurements")?;

    while let Some(notification) = notifications.next().await {
        if notification.uuid != HEART_RATE_MEASUREMENT_UUID {
            continue;
        }
        let bpm = parse_heart_rate_measurement(&notification.value)?;
        events
            .send(BleHeartRateEvent::Measurement(bpm))
            .await
            .map_err(|_| anyhow!("heart-rate event consumer stopped"))?;
    }

    let _ = events.send(BleHeartRateEvent::Disconnected).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example_device() -> BleDeviceInfo {
        BleDeviceInfo {
            id: "DeviceId(AA:BB:CC:DD:EE:FF)".into(),
            address: "AA:BB:CC:DD:EE:FF".into(),
            name: Some("Polar H10 1234".into()),
        }
    }

    #[test]
    fn parses_eight_bit_measurement() {
        assert_eq!(parse_heart_rate_measurement(&[0x00, 57]).unwrap(), 57);
    }

    #[test]
    fn parses_sixteen_bit_measurement() {
        assert_eq!(
            parse_heart_rate_measurement(&[0x01, 0x2C, 0x01]).unwrap(),
            300
        );
    }

    #[test]
    fn ignores_optional_fields_after_bpm() {
        assert_eq!(
            parse_heart_rate_measurement(&[0b0001_1110, 72, 0xAA, 0xBB, 0xCC]).unwrap(),
            72
        );
    }

    #[test]
    fn rejects_truncated_measurements() {
        assert!(parse_heart_rate_measurement(&[]).is_err());
        assert!(parse_heart_rate_measurement(&[0x00]).is_err());
        assert!(parse_heart_rate_measurement(&[0x01, 0x34]).is_err());
    }

    #[test]
    fn device_filter_matches_id_address_or_name_case_insensitively() {
        let device = example_device();
        assert!(device.matches_filter("deviceid"));
        assert!(device.matches_filter("bb:cc:dd"));
        assert!(device.matches_filter("polar h10"));
        assert!(device.matches_filter("  POLAR H10  "));
        assert!(!device.matches_filter("garmin"));
    }

    #[test]
    fn empty_device_filter_matches_any_device() {
        assert!(example_device().matches_filter("  "));
    }

    #[test]
    fn device_lists_put_named_devices_first_and_deduplicate_ids() {
        let mut devices = vec![
            BleDeviceInfo {
                id: "unnamed".into(),
                address: "00:00:00:00:00:03".into(),
                name: None,
            },
            BleDeviceInfo {
                id: "polar".into(),
                address: "00:00:00:00:00:02".into(),
                name: Some("Polar H10".into()),
            },
            BleDeviceInfo {
                id: "polar".into(),
                address: "00:00:00:00:00:02".into(),
                name: Some("Polar H10".into()),
            },
            BleDeviceInfo {
                id: "garmin".into(),
                address: "00:00:00:00:00:01".into(),
                name: Some("Garmin HRM".into()),
            },
        ];

        sort_and_dedup_devices(&mut devices);

        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].name.as_deref(), Some("Garmin HRM"));
        assert_eq!(devices[1].name.as_deref(), Some("Polar H10"));
        assert_eq!(devices[2].name, None);
    }
}
