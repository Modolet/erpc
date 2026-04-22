use crate::generated::{
    telemetry_server::TelemetryClient, Calibration, DeviceReport, Reading, SensorKind, StatusCode,
};
use erpc_rust::{codec::BasicCodecFactory, transport::BlockingTransport, BlockingClientManager};

pub fn run_client<T>(
    manager: &mut BlockingClientManager<T, BasicCodecFactory>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    T: BlockingTransport,
{
    let mut client = TelemetryClient::new(manager);
    let calibration = Calibration {
        operator_name: "factory-line-a".to_string(),
        offsets: vec![0.1, -0.05, 0.2],
        certificate: vec![0xde, 0xad, 0xbe, 0xef],
    };

    let report = DeviceReport {
        device_id: "sensor-hub-17".to_string(),
        readings: vec![
            Reading {
                id: 1,
                kind: SensorKind::Temperature,
                value: 24.25,
                timestamp: 1_700_010_001,
            },
            Reading {
                id: 2,
                kind: SensorKind::Pressure,
                value: 99.75,
                timestamp: 1_700_010_002,
            },
        ],
        calibration: calibration.clone(),
        sequence: 7,
    };

    let status = client.submit_report(report)?;
    assert_eq!(status, StatusCode::Ok);

    let (status, readings) = client.fetch_readings(
        "sensor-hub-17",
        vec![SensorKind::Temperature, SensorKind::Voltage],
    )?;
    assert_eq!(status, StatusCode::Ok);
    assert_eq!(readings.len(), 1);
    assert_eq!(readings[0].kind, SensorKind::Temperature);

    let calibrated = client.calibrate("sensor-hub-17", calibration)?;
    assert_eq!(calibrated.sequence, 42);
    assert_eq!(calibrated.readings.len(), 3);
    assert_eq!(
        calibrated.calibration.certificate,
        vec![0xde, 0xad, 0xbe, 0xef]
    );

    client.publish_heartbeat("sensor-hub-17", 1_700_020_000)?;

    println!("submit_report: {:?}", status);
    println!("fetch_readings: {:?}", readings);
    println!("calibrate: {:?}", calibrated);
    Ok(())
}
