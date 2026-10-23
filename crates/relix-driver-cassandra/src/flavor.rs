/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::DriverId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Cassandra,
    ScyllaDb,
}

impl Flavor {
    pub fn driver_id(self) -> &'static str {
        match self {
            Self::Cassandra => DriverId::CASSANDRA,
            Self::ScyllaDb => DriverId::SCYLLADB,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Cassandra => "Apache Cassandra",
            Self::ScyllaDb => "ScyllaDB",
        }
    }
}
