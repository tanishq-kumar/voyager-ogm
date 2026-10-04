#![no_main]

use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use voyager_net::redis::RespValue;

fuzz_target!(|data: &[u8]| {
    let mut buf = BytesMut::from(data);
    // RespValue::parse must be completely resilient against arbitrary untrusted bytes:
    // It should either return Ok(Some(val)), Ok(None), or Err(NetError::ProtocolError), never panic or leak memory.
    let _ = RespValue::parse(&mut buf);
});
