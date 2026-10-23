/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use super::connections::human_size;
use crate::events::AppEvent;
use crate::notifications::Level;
use crate::overlay::{
    ConfirmState, Overlay, PendingAction, PickerItem, PickerPurpose, PickerState, PickerValue,
};
use crate::services::InstallOffer;
use relix_config::{KeyChord, KeyCode};
use relix_core::{DataSourceId, DriverId};

impl App {
    pub(crate) fn open_driver_manager(&mut self) {
        let items = self
            .context
            .drivers
            .descriptors()
            .into_iter()
            .map(|driver| PickerItem {
                label: driver.name.clone(),
                detail: format!("{} · v{}", driver.origin, driver.version),
                hint: driver.status.clone(),
                value: PickerValue::Driver(driver.id),
            })
            .collect();
        self.overlays.push(Overlay::Picker(PickerState::new(
            "Driver manager",
            PickerPurpose::Drivers,
            items,
            "Ctrl+U check for updates/new packages · Ctrl+D remove external driver · Esc close",
        )));
    }

    pub(crate) fn driver_picker_shortcut(
        &mut self,
        picker: PickerState,
        chord: KeyChord,
    ) -> Option<Overlay> {
        let selected = picker.selected_item().and_then(|item| match &item.value {
            PickerValue::Driver(id) => Some(id.clone()),
            _ => None,
        });
        match (chord.code, selected) {
            (KeyCode::Char('u'), _) => {
                self.check_driver_updates();
                Some(Overlay::Picker(picker))
            }
            (KeyCode::Char('d'), Some(id)) => {
                let external = self
                    .context
                    .drivers
                    .descriptors()
                    .into_iter()
                    .any(|driver| driver.id == id && driver.origin != "built-in");
                if !external {
                    self.notify(
                        Level::Info,
                        "built-in drivers are compiled into RELIX and cannot be removed",
                    );
                    return Some(Overlay::Picker(picker));
                }
                let in_use = self.project.as_ref().is_some_and(|project| {
                    project
                        .data_sources
                        .iter()
                        .any(|source| source.driver == id)
                });
                Some(Overlay::Confirm(ConfirmState {
                    title: format!("Remove driver '{id}'?"),
                    lines: if in_use {
                        vec!["Data sources in this project still use this driver.".into()]
                    } else {
                        vec!["The connector package is deleted from the drivers directory.".into()]
                    },
                    confirm_label: "Remove".into(),
                    danger: true,
                    action: PendingAction::RemoveDriver(id),
                }))
            }
            _ => Some(Overlay::Picker(picker)),
        }
    }

    pub(crate) fn driver_selected(&mut self, id: &DriverId) {
        let descriptor = self
            .context
            .drivers
            .descriptors()
            .into_iter()
            .find(|driver| &driver.id == id);
        if let Some(driver) = descriptor {
            let capabilities = driver.capabilities;
            let text = format!(
                "driver        {}\nid            {}\nversion       {}\norigin        {}\nstatus        {}\ndialect       {}\nmulti-db      {}\nschemas       {}\ntransactions  {}\ncancellation  {}\nediting       {}\ntls           {}",
                driver.name,
                driver.id,
                driver.version,
                driver.origin,
                driver.status,
                driver.dialect.map_or("—", |dialect| dialect.name()),
                capabilities.multiple_databases,
                capabilities.schemas,
                capabilities.transactions,
                capabilities.cancellation,
                capabilities.table_editing,
                capabilities.tls,
            );
            self.overlays
                .push(Overlay::Viewer(crate::overlay::ViewerState::new(
                    "Driver", &text,
                )));
        }
    }

    fn check_driver_updates(&mut self) {
        let service = self.context.drivers.clone();
        self.status = "checking driver registries…".into();
        self.spawn(async move { AppEvent::DriverUpdates(service.updates().await) });
    }

    pub(crate) fn on_driver_updates(&mut self, result: Result<Vec<InstallOffer>, String>) {
        match result {
            Ok(offers) if offers.is_empty() => {
                self.notify(Level::Info, "all drivers are up to date")
            }
            Ok(mut offers) => {
                let offer = offers.remove(0);
                if !offers.is_empty() {
                    self.notify(
                        Level::Info,
                        format!("{} more driver package(s) available", offers.len()),
                    );
                }
                self.confirm_driver_install(offer, None);
            }
            Err(error) => self.notify(
                Level::Error,
                format!("driver registry check failed: {error}"),
            ),
        }
    }

    pub(crate) fn install_driver(&mut self, offer: InstallOffer, retry: Option<DataSourceId>) {
        let service = self.context.drivers.clone();
        self.notify(
            Level::Info,
            format!(
                "installing {} v{} ({})…",
                offer.name,
                offer.version,
                human_size(offer.size_bytes)
            ),
        );
        let driver = offer.id.clone();
        self.spawn(async move {
            let result = service.install(&driver).await;
            AppEvent::DriverInstalled {
                driver,
                retry,
                result,
            }
        });
    }

    pub(crate) fn on_driver_installed(
        &mut self,
        driver: DriverId,
        retry: Option<DataSourceId>,
        result: Result<String, String>,
    ) {
        match result {
            Ok(version) => {
                self.notify(
                    Level::Success,
                    format!("installed driver {driver} v{version}"),
                );
                self.refresh_explorer_roots();
                if let Some(source) = retry {
                    self.connect(source);
                }
            }
            Err(error) => self.notify(Level::Error, format!("driver installation failed: {error}")),
        }
    }

    pub(crate) fn remove_driver(&mut self, id: &DriverId) {
        match self.context.drivers.remove(id) {
            Ok(()) => self.notify(Level::Success, format!("removed driver {id}")),
            Err(error) => self.notify(Level::Error, error),
        }
    }
}
