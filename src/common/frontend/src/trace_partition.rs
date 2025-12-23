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

use std::collections::HashMap;

use api::v1::ColumnDef;
use common_catalog::consts::{PARENT_SPAN_ID_COLUMN, SERVICE_NAME_COLUMN, TRACE_ID_COLUMN};
use sql::statements::create::Partitions;

pub fn trace_partition_rule(_col_defs: &[ColumnDef]) -> Result<Partitions, sql::error::Error> {
    #[cfg(not(feature = "enterprise"))]
    let p = sql::partition::partition_rule_for_hexstring(TRACE_ID_COLUMN);
    #[cfg(feature = "enterprise")]
    let p = {
        use common_catalog::consts::TRACE_HASH_COLUMN;
        use sql::ast::Ident;
        if _col_defs.iter().any(|c| c.name == TRACE_HASH_COLUMN) {
            use sql::partition::partition_rules_for_u64;
            Ok(Partitions {
                column_list: vec![Ident::new(TRACE_HASH_COLUMN)],
                exprs: partition_rules_for_u64(TRACE_HASH_COLUMN),
            })
        } else {
            // TRACE_ID_COLUMN
            use sql::partition::partition_rule_for_hexstring;
            partition_rule_for_hexstring(TRACE_ID_COLUMN)
        }
    };

    p
}

pub fn append_trace_option() -> HashMap<String, String> {
    let mut table_options = HashMap::new();
    #[cfg(feature = "enterprise")]
    table_options.insert(
        table::requests::TRACE_DATA_HASH.to_string(),
        "true".to_string(),
    );

    table_options
}

pub fn index_columns<'a>(_col_defs: &[ColumnDef]) -> Vec<&'a str> {
    #[cfg(not(feature = "enterprise"))]
    let columns = vec![TRACE_ID_COLUMN, PARENT_SPAN_ID_COLUMN, SERVICE_NAME_COLUMN];
    #[cfg(feature = "enterprise")]
    let columns = {
        use common_catalog::consts::TRACE_HASH_COLUMN;
        if _col_defs.iter().any(|c| c.name == TRACE_HASH_COLUMN) {
            vec![
                TRACE_ID_COLUMN,
                PARENT_SPAN_ID_COLUMN,
                SERVICE_NAME_COLUMN,
                TRACE_HASH_COLUMN,
            ]
        } else {
            vec![TRACE_ID_COLUMN, PARENT_SPAN_ID_COLUMN, SERVICE_NAME_COLUMN]
        }
    };

    columns
}
