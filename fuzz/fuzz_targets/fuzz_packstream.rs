#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use voyager_net::bolt::PackStream;

fuzz_target!(|data: &[u8]| {
    let mut bytes = Bytes::copy_from_slice(data);
    // PackStream::decode must be completely resilient against arbitrary untrusted bytes:
    // It should either return Ok(val) or Err(NetError::ProtocolError), never panic or leak memory.
    let _ = PackStream::decode(&mut bytes);
});
