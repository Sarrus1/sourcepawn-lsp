use fxhash::FxHashMap;
use lsp_types::{Position, Range};
use sourcepawn_lexer::{Comment, Literal, Operator, Symbol, TokenKind};

use super::{
    errors::{ExpansionError, MacroNotFoundError, ParseIntError},
    preprocessor::Macro,
};

pub(super) fn expand_symbol<T>(
    lexer: &mut T,
    macros: &FxHashMap<String, Macro>,
    symbol: &Symbol,
    expansion_stack: &mut Vec<Symbol>,
    allow_undefined_macros: bool,
) -> Result<Vec<Symbol>, ExpansionError>
where
    T: Iterator<Item = Symbol>,
{
    let mut expanded_macros = vec![];
    let depth = 0;
    let mut stack: Vec<(Symbol, sourcepawn_lexer::Delta, i32)> =
        vec![(symbol.clone(), symbol.delta, depth)];
    let mut args_stack = vec![];
    while let Some((symbol, delta, d)) = stack.pop() {
        if d == 5 {
            continue;
        }
        match &symbol.token_kind {
            TokenKind::Identifier => {
                let macro_ = macros.get(&symbol.text());
                if macro_.is_none() {
                    if !allow_undefined_macros {
                        return Err(MacroNotFoundError::new(symbol.text(), symbol.range).into());
                    }
                    let mut symbol = symbol.clone();
                    symbol.delta = delta;
                    expansion_stack.push(symbol);
                    continue;
                }
                let macro_ = macro_.unwrap();
                if d == 0 {
                    // Do not keep track of sub-macros, they will not appear in the final document.
                    expanded_macros.push(symbol.clone());
                }
                if macro_.args.is_none() {
                    expand_non_macro_define(macro_, &mut stack, &symbol, d);
                } else {
                    let args = collect_arguments(lexer, &mut args_stack);
                    expand_macro(args, macro_, &mut stack, &symbol, d)?;
                }
            }
            TokenKind::Literal(Literal::StringLiteral)
            | TokenKind::Literal(Literal::CharLiteral) => {
                let text = symbol.inline_text();
                expansion_stack.push(Symbol::new(
                    symbol.token_kind.clone(),
                    Some(&text),
                    Range::new(
                        symbol.range.start,
                        Position::new(
                            symbol.range.start.line,
                            symbol.range.start.character + text.len() as u32,
                        ),
                    ),
                    symbol.delta,
                ));
            }
            TokenKind::Newline
            | TokenKind::LineContinuation
            | TokenKind::Comment(Comment::LineComment) => (),
            _ => {
                let mut symbol = symbol.clone();
                symbol.delta = delta;
                expansion_stack.push(symbol);
            }
        }
    }

    Ok(expanded_macros)
}

fn expand_non_macro_define(
    macro_: &Macro,
    stack: &mut Vec<(Symbol, sourcepawn_lexer::Delta, i32)>,
    symbol: &Symbol,
    d: i32,
) {
    for (i, child) in macro_.body.iter().enumerate() {
        stack.push((
            child.clone(),
            if i == 0 { symbol.delta } else { child.delta },
            d + 1,
        ));
    }
}

fn expand_macro(
    args: Vec<Vec<Symbol>>,
    macro_: &Macro,
    stack: &mut Vec<(Symbol, sourcepawn_lexer::Delta, i32)>,
    symbol: &Symbol,
    d: i32,
) -> Result<(), ParseIntError> {
    let mut consecutive_percent = 0;
    for (i, child) in macro_.body.iter().enumerate() {
        match &child.token_kind {
            TokenKind::Operator(Operator::Percent) => {
                // Count consecutive % tokens.
                // Keep every odd number and if a literal is found, pop the stack to remove it
                // and insert the argument instead.
                // This allows to preserve the spacing between the last token and the % when
                // there is an escaped %.
                consecutive_percent += 1;
                if consecutive_percent % 2 == 1 {
                    stack.push((child.clone(), child.delta, d + 1))
                }
            }
            TokenKind::Literal(Literal::IntegerLiteral) => {
                if consecutive_percent == 1 {
                    stack.pop();
                    let arg_idx = child
                        .to_int()
                        .ok_or_else(|| ParseIntError::new(child.text(), child.range))?
                        as usize;
                    if arg_idx >= 10 {
                        return Err(ParseIntError::new(child.text(), child.range));
                    }
                    // Safe to unwrap here because we know the macro has arguments.
                    let arg_idx = macro_.args.as_ref().unwrap()[arg_idx];
                    if arg_idx >= 10 {
                        return Err(ParseIntError::new(child.text(), child.range));
                    }
                    for (i, child) in args[arg_idx as usize].iter().enumerate() {
                        stack.push((
                            child.clone(),
                            if i == 0 { symbol.delta } else { child.delta },
                            d + 1,
                        ));
                    }
                } else {
                    stack.push((child.clone(), child.delta, d + 1));
                }
                consecutive_percent = 0;
            }
            _ => {
                stack.push((
                    child.clone(),
                    if i == 0 { symbol.delta } else { child.delta },
                    d + 1,
                ));
                consecutive_percent = 0;
            }
        }
    }

    Ok(())
}

/// Assuming we are right before a macro call in the lexer, collect the arguments
/// and store them in an array, in the order they appear in.
///
/// # Arguments
///
/// * `lexer` - [SourcepawnLexer](sourcepawn_lexer::lexer) to iterate over.
fn collect_arguments<T>(lexer: &mut T, args_stack: &mut Vec<Symbol>) -> Vec<Vec<Symbol>>
where
    T: Iterator<Item = Symbol>,
{
    let mut paren_depth = 0;
    let mut arg_idx = 0;
    let mut args: Vec<Vec<Symbol>> = vec![];
    for _ in 0..10 {
        args.push(vec![]);
    }
    let mut new_args_stack = vec![];
    while let Some(sub_symbol) = if !args_stack.is_empty() {
        args_stack.pop()
    } else {
        lexer.next()
    } {
        match &sub_symbol.token_kind {
            TokenKind::LParen => {
                paren_depth += 1;
            }
            TokenKind::RParen => {
                if paren_depth > 1 {
                    new_args_stack.push(sub_symbol.clone());
                }
                paren_depth -= 1;
                if paren_depth == 0 {
                    break;
                }
            }
            TokenKind::Comma => {
                if paren_depth == 1 {
                    arg_idx += 1;
                }
            }
            _ => {
                if paren_depth == 1 {
                    args[arg_idx].push(sub_symbol.clone());
                }
            }
        }
        if paren_depth > 1 {
            new_args_stack.push(sub_symbol.clone());
        }
    }
    new_args_stack.reverse();
    args_stack.extend(new_args_stack);

    args
}
