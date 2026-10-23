/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::form::state::{FieldValue, FormStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormRow<'a> {
    pub label: &'static str,
    pub value: &'a FieldValue,
    pub focused: bool,
    pub placeholder: Option<String>,
}

pub trait FormView {
    fn title(&self) -> String;

    fn rows(&self) -> Vec<FormRow<'_>>;

    fn status(&self) -> Option<&FormStatus>;

    fn busy(&self) -> bool;

    fn footer(&self) -> &'static str;
}
