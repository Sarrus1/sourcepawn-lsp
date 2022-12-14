use std::sync::{Arc, Mutex};

use super::Location;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemTag, CompletionParams, GotoDefinitionParams,
    Hover, HoverContents, HoverParams, LanguageString, LocationLink, MarkedString, Range, Url,
};

use crate::providers::hover::description::Description;

use super::SPItem;

#[derive(Debug, Clone)]
/// SPItem representation of a SourcePawn property, which can be converted to a
/// [CompletionItem](lsp_types::CompletionItem), [Location](lsp_types::Location), etc.
pub struct PropertyItem {
    /// Name of the property.
    pub name: String,

    /// Parent of the property.
    pub parent: Arc<Mutex<SPItem>>,

    /// Type of the property.
    pub type_: String,

    /// Range of the name of the property.
    pub range: Range,

    /// Range of the whole property, including its block.
    pub full_range: Range,

    /// Description of the property.
    pub description: Description,

    /// Uri of the file where the property is declared.
    pub uri: Arc<Url>,

    /// References to this property.
    pub references: Vec<Location>,
}

impl PropertyItem {
    fn is_deprecated(&self) -> bool {
        self.description.deprecated.is_some()
    }

    /// Return a [CompletionItem](lsp_types::CompletionItem) from a [PropertyItem].
    ///
    /// # Arguments
    ///
    /// * `params` - [CompletionParams](lsp_types::CompletionParams) of the request.
    pub(crate) fn to_completion(
        &self,
        _params: &CompletionParams,
        request_method: bool,
    ) -> Option<CompletionItem> {
        // Don't return a property if non method items are requested.
        if !request_method {
            return None;
        }

        let mut tags = vec![];
        if self.is_deprecated() {
            tags.push(CompletionItemTag::DEPRECATED);
        }

        Some(CompletionItem {
            label: self.name.to_string(),
            kind: Some(CompletionItemKind::PROPERTY),
            tags: Some(tags),
            detail: Some(self.parent.lock().unwrap().name()),
            deprecated: Some(self.is_deprecated()),
            ..Default::default()
        })
    }

    /// Return a [Hover] from a [PropertyItem].
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

    /// Return a [LocationLink] from a [PropertyItem].
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

    /// Formatted representation of a [PropertyItem].
    ///
    /// # Exemple
    ///
    /// `void OnPluginStart()`
    fn formatted_text(&self) -> MarkedString {
        MarkedString::LanguageString(LanguageString {
            language: "sourcepawn".to_string(),
            value: format!("{} {}", self.parent.lock().unwrap().name(), self.name),
        })
    }
}
