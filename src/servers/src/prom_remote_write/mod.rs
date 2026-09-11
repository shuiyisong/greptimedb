// Copyright 2023 Greptime Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Prometheus remote write support.
//!
//! This module groups validation, row building, and protobuf decoding for
//! the Prometheus remote write API.

pub mod decode;
pub(crate) mod row_builder;
pub(crate) mod types;
#[cfg(any(test, feature = "testing"))]
pub mod v2;
#[cfg(not(any(test, feature = "testing")))]
pub(crate) mod v2;
pub mod validation;

use bytes::Bytes;
use common_telemetry::{debug, tracing};
use lazy_static::lazy_static;
use object_pool::Pool;
use snafu::ResultExt;

use crate::error;
use crate::prom_remote_write::decode::{PromSeriesProcessor, PromWriteRequest};
use crate::prom_remote_write::row_builder::TablesBuilder;
use crate::prom_remote_write::validation::PromValidationMode;
use crate::prom_store::{snappy_decompress, zstd_decompress};

lazy_static! {
    static ref PROM_WRITE_REQUEST_POOL: Pool<PromWriteRequest<'static>> =
        Pool::new(256, PromWriteRequest::default);
}

pub fn try_decompress(is_zstd: bool, body: &[u8]) -> crate::error::Result<Vec<u8>> {
    if is_zstd {
        zstd_decompress(body)
    } else {
        snappy_decompress(body)
    }
}

/// Logs the decoded remote write request body at the debug level.
///
/// The decoders are row-oriented and do not keep the wire message around, so
/// this makes an extra pass over the already decompressed payload to rebuild it.
/// That extra pass only runs when debug logging is enabled.
pub(crate) fn log_decoded_write_request<M>(version: &str, buf: &[u8])
where
    M: prost::Message + std::fmt::Debug + Default,
{
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }

    match M::decode(buf) {
        Ok(request) => debug!("Prometheus remote write v{version} request body: {request:?}"),
        Err(err) => debug!(
            "Failed to decode Prometheus remote write v{version} request body for logging: {err:?}"
        ),
    }
}

pub fn decode_remote_write_request(
    is_zstd: bool,
    body: Bytes,
    prom_validation_mode: PromValidationMode,
    processor: &mut PromSeriesProcessor,
) -> crate::error::Result<TablesBuilder<'static>> {
    let _timer = crate::metrics::METRIC_HTTP_PROM_STORE_DECODE_ELAPSED.start_timer();

    // due to vmagent's limitation, there is a chance that vmagent is
    // sending content type wrong so we have to apply a fallback with decoding
    // the content in another method.
    //
    // see https://github.com/VictoriaMetrics/VictoriaMetrics/issues/5301
    // see https://github.com/GreptimeTeam/greptimedb/issues/3929
    let buf = if let Ok(buf) = try_decompress(is_zstd, &body[..]) {
        buf
    } else {
        // fallback to the other compression method
        try_decompress(!is_zstd, &body[..])?
    };

    log_decoded_write_request::<api::prom_store::remote::WriteRequest>("1.0", &buf);

    let mut request = PROM_WRITE_REQUEST_POOL.pull(PromWriteRequest::default);

    request
        .decode(buf, prom_validation_mode, processor)
        .context(error::DecodePromRemoteRequestSnafu)?;
    Ok(std::mem::take(&mut request.table_data))
}
