use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use lsp_types::{
    CompletionItem, CompletionParams, GotoDefinitionParams, Hover, HoverParams, LocationLink,
    Position, Range, Url,
};

use crate::{
    document::Document,
    providers::hover::description::Description,
    store::Store,
    utils::{range_contains_pos, range_equals_range},
};

pub mod define_item;
pub mod enum_item;
pub mod enum_member_item;
pub mod enum_struct_item;
pub mod function_item;
pub mod include_item;
pub mod methodmap_item;
pub mod property_item;
pub mod variable_item;

/// Represents a location inside a resource, such as a line inside a text file.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Location {
    pub uri: Arc<Url>,
    pub range: Range,
}

#[derive(Debug, Clone)]
/// Generic representation of an item, which can be converted to a
/// [CompletionItem](lsp_types::CompletionItem), [Location](lsp_types::Location), etc.
pub enum SPItem {
    Function(function_item::FunctionItem),
    Variable(variable_item::VariableItem),
    Enum(enum_item::EnumItem),
    EnumMember(enum_member_item::EnumMemberItem),
    EnumStruct(enum_struct_item::EnumStructItem),
    Define(define_item::DefineItem),
    Methodmap(methodmap_item::MethodmapItem),
    Property(property_item::PropertyItem),
    Include(include_item::IncludeItem),
}

pub fn get_all_items(store: &Store) -> Vec<Arc<Mutex<SPItem>>> {
    let mut all_items = vec![];
    if let Some(main_path_uri) = store.environment.options.get_main_path_uri() {
        let mut includes: HashSet<Url> = HashSet::new();
        includes.insert(main_path_uri.clone());
        if let Some(document) = store.documents.get(&main_path_uri) {
            get_included_files(store, document, &mut includes);
            for include in includes.iter() {
                let document = store.documents.get(include).unwrap();
                for item in document.sp_items.iter() {
                    all_items.push(item.clone());
                }
            }
        }
        return all_items;
    }
    for document in store.documents.values() {
        for item in document.sp_items.iter() {
            all_items.push(item.clone());
        }
    }

    all_items
}

fn get_included_files(store: &Store, document: &Document, includes: &mut HashSet<Url>) {
    for include_uri in document.includes.iter() {
        if includes.contains(include_uri) {
            continue;
        }
        includes.insert(include_uri.clone());
        if let Some(include_document) = store.get(include_uri) {
            get_included_files(store, &include_document, includes);
        }
    }
}

pub fn get_items_from_position(
    store: &Store,
    position: Position,
    uri: Url,
) -> Vec<Arc<Mutex<SPItem>>> {
    let uri = Arc::new(uri);
    let all_items = get_all_items(store);
    let mut res = vec![];
    for item in all_items.iter() {
        let item_lock = item.lock().unwrap();
        match item_lock.range() {
            Some(range) => {
                if range_contains_pos(range, position) && item_lock.uri().as_ref().eq(&uri) {
                    res.push(item.clone());
                    continue;
                }
            }
            None => {
                continue;
            }
        }
        match item_lock.references() {
            Some(references) => {
                for reference in references.iter() {
                    if range_contains_pos(reference.range, position) && reference.uri.eq(&uri) {
                        res.push(item.clone());
                        break;
                    }
                }
            }
            None => {
                continue;
            }
        }
    }
    res
}

impl SPItem {
    pub fn range(&self) -> Option<Range> {
        match self {
            SPItem::Variable(item) => Some(item.range),
            SPItem::Function(item) => Some(item.range),
            SPItem::Enum(item) => Some(item.range),
            SPItem::EnumMember(item) => Some(item.range),
            SPItem::EnumStruct(item) => Some(item.range),
            SPItem::Define(item) => Some(item.range),
            SPItem::Methodmap(item) => Some(item.range),
            SPItem::Property(item) => Some(item.range),
            SPItem::Include(item) => Some(item.range),
        }
    }

    pub fn full_range(&self) -> Option<Range> {
        match self {
            SPItem::Variable(item) => Some(item.range),
            SPItem::Function(item) => Some(item.full_range),
            SPItem::Enum(item) => Some(item.full_range),
            SPItem::EnumMember(item) => Some(item.range),
            SPItem::EnumStruct(item) => Some(item.full_range),
            SPItem::Define(item) => Some(item.full_range),
            SPItem::Methodmap(item) => Some(item.full_range),
            SPItem::Property(item) => Some(item.full_range),
            SPItem::Include(item) => Some(item.range),
        }
    }

    pub fn name(&self) -> String {
        match self {
            SPItem::Variable(item) => item.name.clone(),
            SPItem::Function(item) => item.name.clone(),
            SPItem::Enum(item) => item.name.clone(),
            SPItem::EnumMember(item) => item.name.clone(),
            SPItem::EnumStruct(item) => item.name.clone(),
            SPItem::Define(item) => item.name.clone(),
            SPItem::Methodmap(item) => item.name.clone(),
            SPItem::Property(item) => item.name.clone(),
            SPItem::Include(item) => item.name.clone(),
        }
    }

    pub fn parent(&self) -> Option<Arc<Mutex<SPItem>>> {
        match self {
            SPItem::Variable(item) => item.parent.clone(),
            SPItem::Function(item) => item.parent.clone(),
            SPItem::Enum(_) => None,
            SPItem::EnumMember(item) => Some(item.parent.clone()),
            SPItem::EnumStruct(_) => None,
            SPItem::Define(_) => None,
            SPItem::Methodmap(_) => None,
            SPItem::Property(item) => Some(item.parent.clone()),
            SPItem::Include(_) => None,
        }
    }

    pub fn type_(&self) -> String {
        match self {
            SPItem::Variable(item) => item.type_.clone(),
            SPItem::Function(item) => item.type_.clone(),
            SPItem::Enum(item) => item.name.clone(),
            SPItem::EnumMember(item) => item.parent.lock().unwrap().name(),
            SPItem::EnumStruct(item) => item.name.clone(),
            SPItem::Define(_) => "".to_string(),
            SPItem::Methodmap(item) => item.name.clone(),
            SPItem::Property(item) => item.type_.clone(),
            SPItem::Include(_) => "".to_string(),
        }
    }

    pub fn description(&self) -> Option<Description> {
        match self {
            SPItem::Variable(item) => Some(item.description.clone()),
            SPItem::Function(item) => Some(item.description.clone()),
            SPItem::Enum(item) => Some(item.description.clone()),
            SPItem::EnumMember(item) => Some(item.description.clone()),
            SPItem::EnumStruct(item) => Some(item.description.clone()),
            SPItem::Define(item) => Some(item.description.clone()),
            SPItem::Methodmap(item) => Some(item.description.clone()),
            SPItem::Property(item) => Some(item.description.clone()),
            SPItem::Include(_) => None,
        }
    }

    pub fn uri(&self) -> Arc<Url> {
        match self {
            SPItem::Variable(item) => item.uri.clone(),
            SPItem::Function(item) => item.uri.clone(),
            SPItem::Enum(item) => item.uri.clone(),
            SPItem::EnumMember(item) => item.uri.clone(),
            SPItem::EnumStruct(item) => item.uri.clone(),
            SPItem::Define(item) => item.uri.clone(),
            SPItem::Methodmap(item) => item.uri.clone(),
            SPItem::Property(item) => item.uri.clone(),
            SPItem::Include(item) => item.uri.clone(),
        }
    }

    pub fn references(&self) -> Option<&Vec<Location>> {
        match self {
            SPItem::Variable(item) => Some(&item.references),
            SPItem::Function(item) => Some(&item.references),
            SPItem::Enum(item) => Some(&item.references),
            SPItem::EnumMember(item) => Some(&item.references),
            SPItem::EnumStruct(item) => Some(&item.references),
            SPItem::Define(item) => Some(&item.references),
            SPItem::Methodmap(item) => Some(&item.references),
            SPItem::Property(item) => Some(&item.references),
            SPItem::Include(_) => None,
        }
    }

    pub fn push_reference(&mut self, reference: Location) {
        if range_equals_range(&self.range().unwrap(), &reference.range)
            && self.uri().eq(&reference.uri)
        {
            return;
        }
        match self {
            SPItem::Variable(item) => item.references.push(reference),
            SPItem::Function(item) => item.references.push(reference),
            SPItem::Enum(item) => item.references.push(reference),
            SPItem::EnumMember(item) => item.references.push(reference),
            SPItem::EnumStruct(item) => item.references.push(reference),
            SPItem::Define(item) => item.references.push(reference),
            SPItem::Methodmap(item) => item.references.push(reference),
            SPItem::Property(item) => item.references.push(reference),
            SPItem::Include(_) => {}
        }
    }

    pub fn set_new_references(&mut self, references: Vec<Location>) {
        match self {
            SPItem::Variable(item) => item.references = references,
            SPItem::Function(item) => item.references = references,
            SPItem::Enum(item) => item.references = references,
            SPItem::EnumMember(item) => item.references = references,
            SPItem::EnumStruct(item) => item.references = references,
            SPItem::Define(item) => item.references = references,
            SPItem::Methodmap(item) => item.references = references,
            SPItem::Property(item) => item.references = references,
            SPItem::Include(_) => {}
        }
    }

    pub fn push_params(&mut self, param: Arc<Mutex<SPItem>>) {
        match self {
            SPItem::Function(item) => item.params.push(param),
            _ => {
                eprintln!("Cannot push params to an item that does not have params.")
            }
        }
    }

    pub fn to_completion(
        &self,
        params: &CompletionParams,
        request_method: bool,
    ) -> Option<CompletionItem> {
        match self {
            SPItem::Variable(item) => item.to_completion(params, request_method),
            SPItem::Function(item) => item.to_completion(params, request_method),
            SPItem::Enum(item) => item.to_completion(params),
            SPItem::EnumMember(item) => item.to_completion(params),
            SPItem::EnumStruct(item) => item.to_completion(params),
            SPItem::Define(item) => item.to_completion(params),
            SPItem::Methodmap(item) => item.to_completion(params),
            SPItem::Property(item) => item.to_completion(params, request_method),
            SPItem::Include(item) => item.to_completion(params),
        }
    }

    pub fn to_hover(&self, params: &HoverParams) -> Option<Hover> {
        match self {
            SPItem::Variable(item) => item.to_hover(params),
            SPItem::Function(item) => item.to_hover(params),
            SPItem::Enum(item) => item.to_hover(params),
            SPItem::EnumMember(item) => item.to_hover(params),
            SPItem::EnumStruct(item) => item.to_hover(params),
            SPItem::Define(item) => item.to_hover(params),
            SPItem::Methodmap(item) => item.to_hover(params),
            SPItem::Property(item) => item.to_hover(params),
            SPItem::Include(item) => item.to_hover(params),
        }
    }

    pub fn to_definition(&self, params: &GotoDefinitionParams) -> Option<LocationLink> {
        match self {
            SPItem::Variable(item) => item.to_definition(params),
            SPItem::Function(item) => item.to_definition(params),
            SPItem::Enum(item) => item.to_definition(params),
            SPItem::EnumMember(item) => item.to_definition(params),
            SPItem::EnumStruct(item) => item.to_definition(params),
            SPItem::Define(item) => item.to_definition(params),
            SPItem::Methodmap(item) => item.to_definition(params),
            SPItem::Property(item) => item.to_definition(params),
            SPItem::Include(item) => item.to_definition(params),
        }
    }
}
