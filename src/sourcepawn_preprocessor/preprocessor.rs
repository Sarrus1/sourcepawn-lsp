use std::sync::Arc;

use anyhow::{anyhow, Context};
use fxhash::FxHashMap;
use lazy_static::lazy_static;
use lsp_types::{Diagnostic, Position, Range, Url};
use regex::Regex;
use sourcepawn_lexer::{Literal, Operator, PreprocDir, SourcepawnLexer, Symbol, TokenKind};

use crate::{document::Token, store::Store};

use super::{
    errors::{EvaluationError, ExpansionError, MacroNotFoundError},
    evaluator::IfCondition,
    macros::expand_symbol,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConditionState {
    NotActivated,
    Activated,
    Active,
}

#[derive(Debug, Clone)]
pub struct SourcepawnPreprocessor<'a> {
    pub(super) lexer: SourcepawnLexer<'a>,
    pub(crate) macros: FxHashMap<String, Macro>,
    pub(super) expansion_stack: Vec<Symbol>,
    skip_line_start_col: u32,
    skipped_lines: Vec<lsp_types::Range>,
    pub(self) macro_not_found_errors: Vec<MacroNotFoundError>,
    pub(self) evaluation_errors: Vec<EvaluationError>,
    pub(self) evaluated_define_symbols: Vec<Symbol>,
    document_uri: Arc<Url>,
    current_line: String,
    prev_end: u32,
    conditions_stack: Vec<ConditionState>,
    out: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Macro {
    pub(crate) args: Option<Vec<i8>>,
    pub(crate) body: Vec<Symbol>,
}

impl<'a> SourcepawnPreprocessor<'a> {
    pub fn new(document_uri: Arc<Url>, input: &'a str) -> Self {
        Self {
            lexer: SourcepawnLexer::new(input),
            document_uri,
            current_line: "".to_string(),
            skip_line_start_col: 0,
            skipped_lines: vec![],
            macro_not_found_errors: vec![],
            evaluation_errors: vec![],
            evaluated_define_symbols: vec![],
            prev_end: 0,
            conditions_stack: vec![],
            out: vec![],
            macros: FxHashMap::default(),
            expansion_stack: vec![],
        }
    }

    pub(crate) fn add_ignored_tokens(&self, tokens: &mut Vec<Arc<Token>>) {
        for symbol in self.evaluated_define_symbols.iter() {
            tokens.push(Arc::new(Token {
                text: symbol.text(),
                range: symbol.range,
            }));
        }
    }

    pub(crate) fn add_diagnostics(&self, diagnostics: &mut Vec<Diagnostic>) {
        self.get_disabled_diagnostics(diagnostics);
        self.get_macro_not_found_diagnostics(diagnostics);
        self.get_evaluation_error_diagnostics(diagnostics);
    }

    fn get_disabled_diagnostics(&self, diagnostics: &mut Vec<Diagnostic>) {
        let mut ranges: Vec<lsp_types::Range> = vec![];
        for range in self.skipped_lines.iter() {
            if let Some(old_range) = ranges.pop() {
                if old_range.end.line == range.start.line - 1 {
                    ranges.push(lsp_types::Range::new(old_range.start, range.end));
                    continue;
                } else {
                    ranges.push(old_range);
                }
            } else {
                ranges.push(*range);
            }
        }
        diagnostics.extend(ranges.iter().map(|range| Diagnostic {
            range: *range,
            message: "Code disabled by the preprocessor.".to_string(),
            severity: Some(lsp_types::DiagnosticSeverity::HINT),
            tags: Some(vec![lsp_types::DiagnosticTag::UNNECESSARY]),
            ..Default::default()
        }));
    }

    fn get_macro_not_found_diagnostics(&self, diagnostics: &mut Vec<Diagnostic>) {
        diagnostics.extend(self.macro_not_found_errors.iter().map(|err| Diagnostic {
            range: err.range,
            message: format!("Macro {} not found.", err.macro_name),
            severity: Some(lsp_types::DiagnosticSeverity::ERROR),
            ..Default::default()
        }));
    }

    fn get_evaluation_error_diagnostics(&self, diagnostics: &mut Vec<Diagnostic>) {
        diagnostics.extend(self.evaluation_errors.iter().map(|err| Diagnostic {
            range: err.range,
            message: format!("Preprocessor condition is invalid: {}", err.text),
            severity: Some(lsp_types::DiagnosticSeverity::ERROR),
            ..Default::default()
        }));
    }

    pub fn preprocess_input(&mut self, store: &mut Store) -> anyhow::Result<String> {
        while let Some(symbol) = if !self.expansion_stack.is_empty() {
            self.expansion_stack.pop()
        } else {
            self.lexer.next()
        } {
            if matches!(
                self.conditions_stack
                    .last()
                    .unwrap_or(&ConditionState::Active),
                ConditionState::Activated | ConditionState::NotActivated
            ) {
                self.process_negative_condition(&symbol)?;
                continue;
            }
            match &symbol.token_kind {
                TokenKind::PreprocDir(dir) => self.process_directive(store, dir, &symbol)?,
                TokenKind::Newline => {
                    self.push_ws(&symbol);
                    self.push_current_line();
                    self.current_line = "".to_string();
                    self.prev_end = 0;
                }
                TokenKind::Identifier => match self.macros.get(&symbol.text()) {
                    // TODO: Evaluate the performance dropoff of supporting macro expansion when overriding reserved keywords.
                    // This might only be a problem for a very small subset of users.
                    Some(_) => {
                        match expand_symbol(
                            &mut self.lexer,
                            &self.macros,
                            &symbol,
                            &mut self.expansion_stack,
                        ) {
                            Ok(_) => continue,
                            Err(ExpansionError::MacroNotFound(err)) => {
                                self.macro_not_found_errors.push(err.clone());
                                return Err(anyhow!("{}", err));
                            }
                            Err(ExpansionError::Parse(err)) => {
                                return Err(anyhow!("{}", err));
                            }
                        }
                    }
                    None => {
                        self.push_symbol(&symbol);
                    }
                },
                TokenKind::Eof => {
                    self.push_ws(&symbol);
                    self.push_current_line();
                    break;
                }
                _ => self.push_symbol(&symbol),
            }
        }

        Ok(self.out.join("\n"))
    }

    fn process_if_directive(&mut self, symbol: &Symbol) {
        let line_nb = symbol.range.start.line;
        let mut if_condition = IfCondition::new(&self.macros, symbol.range.start.line);
        while self.lexer.in_preprocessor() {
            if let Some(symbol) = self.lexer.next() {
                if symbol.token_kind == TokenKind::Identifier {
                    self.evaluated_define_symbols.push(symbol.clone());
                }
                if_condition.symbols.push(symbol);
            } else {
                break;
            }
        }
        let if_condition_eval = match if_condition.evaluate() {
            Ok(res) => res,
            Err(err) => {
                self.evaluation_errors.push(err);
                // Default to false when we fail to evaluate a condition.
                false
            }
        };

        if if_condition_eval {
            self.conditions_stack.push(ConditionState::Active);
        } else {
            self.skip_line_start_col = symbol.range.end.character;
            self.conditions_stack.push(ConditionState::NotActivated);
        }
        self.macro_not_found_errors
            .extend(if_condition.macro_not_found_errors);
        if let Some(last_symbol) = if_condition.symbols.last() {
            let line_diff = last_symbol.range.end.line - line_nb;
            for _ in 0..line_diff {
                self.out.push(String::new());
            }
        }

        self.prev_end = 0;
    }

    fn process_else_directive(&mut self, symbol: &Symbol) -> anyhow::Result<()> {
        let last = self
            .conditions_stack
            .pop()
            .context("Expect if before else clause.")?;
        match last {
            ConditionState::NotActivated => {
                self.conditions_stack.push(ConditionState::Active);
            }
            ConditionState::Active | ConditionState::Activated => {
                self.skip_line_start_col = symbol.range.end.character;
                self.conditions_stack.push(ConditionState::Activated);
            }
        }

        Ok(())
    }

    fn process_endif_directive(&mut self, symbol: &Symbol) -> anyhow::Result<()> {
        self.conditions_stack
            .pop()
            .context("Expect if before endif clause")?;
        if let Some(last) = self.conditions_stack.last() {
            if *last != ConditionState::Active {
                self.skipped_lines.push(lsp_types::Range::new(
                    Position::new(symbol.range.start.line, self.skip_line_start_col),
                    Position::new(symbol.range.start.line, symbol.range.end.character),
                ));
            }
        }

        Ok(())
    }

    fn process_directive(
        &mut self,
        store: &mut Store,
        dir: &PreprocDir,
        symbol: &Symbol,
    ) -> anyhow::Result<()> {
        match dir {
            PreprocDir::MIf => self.process_if_directive(symbol),
            PreprocDir::MElseif => {
                let last = self
                    .conditions_stack
                    .pop()
                    .context("Expect if before elseif clause.")?;
                match last {
                    ConditionState::NotActivated => self.process_if_directive(symbol),
                    ConditionState::Active | ConditionState::Activated => {
                        self.conditions_stack.push(ConditionState::Activated);
                    }
                }
            }
            PreprocDir::MDefine => {
                self.push_symbol(symbol);
                let mut macro_name = String::new();
                let mut macro_ = Macro {
                    args: None,
                    body: vec![],
                };
                enum State {
                    Start,
                    Args,
                    Body,
                }
                let mut args = vec![-1, 10];
                let mut found_args = false;
                let mut state = State::Start;
                let mut args_idx = 0;
                while self.lexer.in_preprocessor() {
                    if let Some(symbol) = self.lexer.next() {
                        self.push_ws(&symbol);
                        self.prev_end = symbol.range.end.character;
                        if !matches!(symbol.token_kind, TokenKind::Newline | TokenKind::Eof) {
                            self.current_line.push_str(&symbol.text());
                        }
                        match state {
                            State::Start => {
                                if macro_name.is_empty()
                                    && TokenKind::Identifier == symbol.token_kind
                                {
                                    macro_name = symbol.text();
                                } else if symbol.delta.col == 0
                                    && symbol.token_kind == TokenKind::LParen
                                {
                                    state = State::Args;
                                } else {
                                    macro_.body.push(symbol);
                                    state = State::Body;
                                }
                            }
                            State::Args => {
                                if symbol.delta.col > 0 {
                                    macro_.body.push(symbol);
                                    state = State::Body;
                                    continue;
                                }
                                match &symbol.token_kind {
                                    TokenKind::RParen => {
                                        state = State::Body;
                                    }
                                    TokenKind::Literal(Literal::IntegerLiteral) => {
                                        found_args = true;
                                        args[symbol.to_int().context(format!(
                                            "Could not convert {:?} to an int value.",
                                            symbol.text()
                                        ))?
                                            as usize] = args_idx;
                                    }
                                    TokenKind::Comma => {
                                        args_idx += 1;
                                    }
                                    TokenKind::Operator(Operator::Percent) => (),
                                    _ => {
                                        return Err(anyhow!(
                                            "Unexpected symbol {} in macro args",
                                            symbol.text()
                                        ))
                                    }
                                }
                            }
                            State::Body => {
                                macro_.body.push(symbol);
                            }
                        }
                    }
                }
                if found_args {
                    macro_.args = Some(args);
                }
                self.push_current_line();
                self.current_line = "".to_string();
                self.prev_end = 0;
                self.macros.insert(macro_name, macro_);
            }
            PreprocDir::MEndif => self.process_endif_directive(symbol)?,
            PreprocDir::MElse => self.process_else_directive(symbol)?,
            PreprocDir::MInclude => {
                let text = symbol.inline_text().trim().to_string();
                let delta = symbol.range.end.line - symbol.range.start.line;
                let symbol = Symbol::new(
                    symbol.token_kind.clone(),
                    Some(&text),
                    Range::new(
                        Position::new(symbol.range.start.line, symbol.range.start.character),
                        Position::new(symbol.range.start.line, text.len() as u32),
                    ),
                    symbol.delta,
                );
                lazy_static! {
                    static ref RE1: Regex = Regex::new(r"<([^>]+)>").unwrap();
                    static ref RE2: Regex = Regex::new("\"([^>]+)\"").unwrap();
                }
                // TODO: Squash this into one regex.
                if let Some(caps) = RE1.captures(&text) {
                    if let Some(path) = caps.get(1) {
                        let mut path = path.as_str().to_string();
                        if let Some(include_uri) =
                            store.resolve_import(&mut path, &self.document_uri)
                        {
                            if let Some(include_macros) =
                                store.preprocess_document_by_uri(Arc::new(include_uri))
                            {
                                self.macros.extend(include_macros);
                            }
                        }
                    }
                };
                if let Some(caps) = RE2.captures(&text) {
                    if let Some(path) = caps.get(1) {
                        let mut path = path.as_str().to_string();
                        if let Some(include_uri) =
                            store.resolve_import(&mut path, &self.document_uri)
                        {
                            if let Some(include_macros) =
                                store.preprocess_document_by_uri(Arc::new(include_uri))
                            {
                                self.macros.extend(include_macros);
                            }
                        }
                    }
                };

                self.push_symbol(&symbol);
                if delta > 0 {
                    self.push_current_line();
                    self.current_line = "".to_string();
                    self.prev_end = 0;
                    for _ in 0..delta - 1 {
                        self.out.push(String::new());
                    }
                }
            }
            _ => self.push_symbol(symbol),
        }

        Ok(())
    }

    fn process_negative_condition(&mut self, symbol: &Symbol) -> anyhow::Result<()> {
        match &symbol.token_kind {
            TokenKind::PreprocDir(dir) => match dir {
                PreprocDir::MIf => {
                    // Keep track of any nested if statements to ensure we properly pop when reaching an endif.
                    self.conditions_stack.push(ConditionState::Activated);
                }
                PreprocDir::MEndif => self.process_endif_directive(symbol)?,
                PreprocDir::MElse => self.process_else_directive(symbol)?,
                PreprocDir::MElseif => {
                    let last = self
                        .conditions_stack
                        .pop()
                        .context("Expect if before elseif clause.")?;
                    match last {
                        ConditionState::NotActivated => self.process_if_directive(symbol),
                        ConditionState::Active | ConditionState::Activated => {
                            self.conditions_stack.push(ConditionState::Activated);
                        }
                    }
                }
                _ => (),
            },
            TokenKind::Newline => {
                // Keep the newline to keep the line numbers in sync.
                self.push_current_line();
                self.skipped_lines.push(lsp_types::Range::new(
                    Position::new(symbol.range.start.line, self.skip_line_start_col),
                    Position::new(symbol.range.start.line, symbol.range.start.character),
                ));
                self.current_line = "".to_string();
                self.prev_end = 0;
            }
            TokenKind::Identifier => {
                // Keep track of the identifiers, so that they can be seen by the semantic analyzer.
                self.evaluated_define_symbols.push(symbol.clone());
            }
            // Skip any token that is not a directive or a newline.
            _ => (),
        }

        Ok(())
    }

    fn push_ws(&mut self, symbol: &Symbol) {
        self.current_line
            .push_str(&" ".repeat(symbol.delta.col.unsigned_abs() as usize));
    }

    fn push_current_line(&mut self) {
        self.out.push(self.current_line.clone());
    }

    fn push_symbol(&mut self, symbol: &Symbol) {
        if symbol.token_kind == TokenKind::Eof {
            self.push_current_line();
            return;
        }
        self.push_ws(symbol);
        self.prev_end = symbol.range.end.character;
        self.current_line.push_str(&symbol.text());
    }
}
