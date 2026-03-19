use erpc_rust::transport::rusb::{
    select_interface_and_endpoints, EndpointAddress, InterfaceCandidate, SelectionError,
};

#[test]
fn selects_first_vendor_interface_with_bulk_endpoints() {
    let interfaces = vec![
        InterfaceCandidate {
            interface_number: 1,
            alternate_setting: 0,
            class_code: 0xff,
            endpoints: vec![
                EndpointAddress::bulk_in(0x81),
                EndpointAddress::bulk_out(0x02),
            ],
        },
        InterfaceCandidate {
            interface_number: 2,
            alternate_setting: 0,
            class_code: 0xff,
            endpoints: vec![
                EndpointAddress::bulk_in(0x83),
                EndpointAddress::bulk_out(0x04),
            ],
        },
    ];

    let selected = select_interface_and_endpoints(&interfaces).expect("should select interface");

    assert_eq!(selected.interface_number, 1);
    assert_eq!(selected.alternate_setting, 0);
    assert_eq!(selected.endpoint_in, 0x81);
    assert_eq!(selected.endpoint_out, 0x02);
}

#[test]
fn errors_when_vendor_interface_lacks_bulk_pair() {
    let interfaces = vec![InterfaceCandidate {
        interface_number: 3,
        alternate_setting: 0,
        class_code: 0xff,
        endpoints: vec![EndpointAddress::bulk_in(0x81)],
    }];

    let err = select_interface_and_endpoints(&interfaces).expect_err("should reject interface");

    assert!(matches!(err, SelectionError::MissingBulkPair));
}
