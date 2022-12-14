use std::sync::{Arc, Mutex};

use super::Location;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, GotoDefinitionParams, Hover,
    HoverContents, HoverParams, LanguageString, LocationLink, MarkedString, Range, Url,
};

use crate::{providers::hover::description::Description, utils::uri_to_file_name};

use super::SPItem;

#[derive(Debug, Clone)]
/// SPItem representation of a SourcePawn methodmap.
pub struct MethodmapItem {
    /// Name of the methodmap.
    pub name: String,

    /// Parent of the methodmap.
    pub parent: Option<Arc<Mutex<SPItem>>>,

    /// Temporary parent of the methodmap.
    pub tmp_parent: Option<String>,

    /// Range of the name of the methodmap.
    pub range: Range,

    /// Range of the whole methodmap, including its value.
    pub full_range: Range,

    /// Description of the methodmap.
    pub description: Description,

    /// Uri of the file where the methodmap is declared.
    pub uri: Arc<Url>,

    /// References to this methodmap.
    pub references: Vec<Location>,
}

impl MethodmapItem {
    /// Return a [CompletionItem](lsp_types::CompletionItem) from an [MethodmapItem].
    ///
    /// # Arguments
    ///
    /// * `_params` - [CompletionParams](lsp_types::CompletionParams) of the request.
    pub(crate) fn to_completion(&self, _params: &CompletionParams) -> Option<CompletionItem> {
        Some(CompletionItem {
            label: self.name.to_string(),
            kind: Some(CompletionItemKind::CLASS),
            detail: uri_to_file_name(&self.uri),
            ..Default::default()
        })
    }

    /// Return a [Hover] from an [MethodmapItem].
    ///
    /// # Arguments
    ///
    /// * `_params` - [HoverParams] of the request.
    pub(crate) fn to_hover(&self, _params: &HoverParams) -> Option<Hover> {
        Some(Hover {
            contents: HoverContents::Array(vec![
                self.formatted_text(),
                MarkedString::String(self.description.to_md()),
            ]),
            range: None,
        })
    }

    /// Return a [LocationLink] from a [MethodmapItem].
    ///
    /// # Arguments
    ///
    /// * `_params` - [GotoDefinitionParams] of the request.
    pub(crate) fn to_definition(&self, _params: &GotoDefinitionParams) -> Option<LocationLink> {
        Some(LocationLink {
            target_range: self.range,
            target_uri: self.uri.as_ref().clone(),
            target_selection_range: self.range,
            origin_selection_range: None,
        })
    }

    /// Formatted representation of the methodmap.
    ///
    /// # Exemple
    ///
    /// `methodmap Foo < Bar`
    fn formatted_text(&self) -> MarkedString {
        let mut suffix = "".to_string();
        if self.parent.is_some() {
            suffix = format!(
                " < {}",
                self.parent.as_ref().unwrap().lock().unwrap().name()
            );
        }
        MarkedString::LanguageString(LanguageString {
            language: "sourcepawn".to_string(),
            value: format!("methodmap {}{}", self.name, suffix)
                .trim()
                .to_string(),
        })
    }
}
