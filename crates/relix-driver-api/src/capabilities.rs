/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::DataModel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverCapabilities {
    pub data_model: DataModel,
    pub multiple_databases: bool,
    pub schemas: bool,
    pub transactions: bool,
    pub cancellation: bool,
    pub table_editing: bool,
    pub routines: bool,
    pub tls: bool,
}

impl DriverCapabilities {
    pub fn relational() -> Self {
        Self {
            data_model: DataModel::Relational,
            multiple_databases: false,
            schemas: false,
            transactions: true,
            cancellation: false,
            table_editing: true,
            routines: false,
            tls: false,
        }
    }

    pub fn key_value() -> Self {
        Self {
            data_model: DataModel::KeyValue,
            multiple_databases: false,
            schemas: false,
            transactions: false,
            cancellation: false,
            table_editing: false,
            routines: false,
            tls: false,
        }
    }

    pub fn is_relational(&self) -> bool {
        self.data_model == DataModel::Relational
    }
}
