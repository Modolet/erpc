use crate::generated::{
    telemetry_server::Telemetry, Calibration, DeviceReport, Reading, SensorKind, StatusCode,
};
use std::sync::Mutex;

pub static LAST_HEARTBEAT: Mutex<Option<(String, u64)>> = Mutex::new(None);

pub struct TelemetryService {
    stored_report: Mutex<Option<DeviceReport>>,
}

impl TelemetryService {
    pub fn new() -> Self {
        Self {
            stored_report: Mutex::new(None),
        }
    }

    fn fallback_reading(device_id: &str, kind: SensorKind, index: usize) -> Reading {
        let base = match kind {
            SensorKind::Temperature => 21.5,
            SensorKind::Pressure => 101.3,
            SensorKind::Voltage => 3.3,
        };

        Reading {
            id: (index + 1) as u32,
            kind,
            value: base + device_id.len() as f32,
            timestamp: 1_700_000_000 + index as u64,
        }
    }
}

impl Telemetry for TelemetryService {
    fn submit_report(
        &self,
        report: DeviceReport,
    ) -> Result<StatusCode, Box<dyn std::error::Error + Send + Sync>> {
        if report.device_id.is_empty() || report.readings.is_empty() {
            return Ok(StatusCode::Invalid);
        }

        let mut stored = self
            .stored_report
            .lock()
            .expect("stored report lock poisoned");
        *stored = Some(report);
        Ok(StatusCode::Ok)
    }

    fn fetch_readings(
        &self,
        device_id: &str,
        kinds: Vec<SensorKind>,
    ) -> Result<(StatusCode, Vec<Reading>), Box<dyn std::error::Error + Send + Sync>> {
        let stored = self
            .stored_report
            .lock()
            .expect("stored report lock poisoned");
        if let Some(report) = stored
            .as_ref()
            .filter(|report| report.device_id == device_id)
        {
            let readings = report
                .readings
                .iter()
                .filter(|reading| kinds.contains(&reading.kind))
                .cloned()
                .collect::<Vec<_>>();
            return Ok((StatusCode::Ok, readings));
        }

        let readings = kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| Self::fallback_reading(device_id, kind, index))
            .collect();
        Ok((StatusCode::NotFound, readings))
    }

    fn calibrate(
        &self,
        device_id: &str,
        calibration: Calibration,
    ) -> Result<DeviceReport, Box<dyn std::error::Error + Send + Sync>> {
        let readings = calibration
            .offsets
            .iter()
            .enumerate()
            .map(|(index, offset)| Reading {
                id: (index + 10) as u32,
                kind: SensorKind::Voltage,
                value: 3.3 + *offset,
                timestamp: 1_700_001_000 + index as u64,
            })
            .collect();

        Ok(DeviceReport {
            device_id: device_id.to_string(),
            readings,
            calibration,
            sequence: 42,
        })
    }

    fn publish_heartbeat(
        &self,
        device_id: &str,
        timestamp: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut heartbeat = LAST_HEARTBEAT.lock().expect("heartbeat lock poisoned");
        *heartbeat = Some((device_id.to_string(), timestamp));
        Ok(())
    }
}
