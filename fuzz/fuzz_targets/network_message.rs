#![no_main]

use axiom_network::NetworkMessage;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = rmp_serde::from_slice::<NetworkMessage>(data);
});
