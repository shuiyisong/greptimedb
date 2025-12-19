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

use common_catalog::consts::{PARENT_SPAN_ID_COLUMN, SERVICE_NAME_COLUMN, TRACE_ID_COLUMN};
use sql::partition::partition_rule_for_hexstring;
use sql::statements::create::Partitions;

#[cfg(feature = "enterprise")]
const ENT_TRACE_PARTITION_COLUMN: &str = "greptime_trace_partition";

pub fn trace_partition_rule() -> Result<Partitions, sql::error::Error> {
    #[cfg(not(feature = "enterprise"))]
    let p = partition_rule_for_hexstring(TRACE_ID_COLUMN);
    #[cfg(feature = "enterprise")]
    let p = partition_rule_for_hexstring(ENT_TRACE_PARTITION_COLUMN);

    p
}

pub fn index_columns<'a>() -> Vec<&'a str> {
    #[cfg(not(feature = "enterprise"))]
    let columns = vec![TRACE_ID_COLUMN, PARENT_SPAN_ID_COLUMN, SERVICE_NAME_COLUMN];

    #[cfg(feature = "enterprise")]
    let columns = vec![
        TRACE_ID_COLUMN,
        ENT_TRACE_PARTITION_COLUMN,
        PARENT_SPAN_ID_COLUMN,
        SERVICE_NAME_COLUMN,
    ];

    columns
}
