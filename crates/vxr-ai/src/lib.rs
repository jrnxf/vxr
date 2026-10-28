//! vxr-ai: the assistant's model clients (`docs/research/ai.md` section 1), with no I/O of
//! their own, so the same code runs natively and in the browser.
//!
//! - [`provider`]: the three clients ([`Kind`]), the models the picker offers and their
//!   prices, and the [`Settings`] a request is sent with.
//! - [`chat`]: the conversation as the app keeps it ([`Item`]) and the request each client
//!   builds from it ([`request`]), replaying each answer as its own provider sent it.
//! - [`stream`]: a [`Decoder`] per client turning the streamed answer into text as it comes and
//!   one [`Turn`] (text, tool calls, usage, why it stopped) at its end.
//! - [`sse`]: server-sent events from bytes in any chunking.
//! - [`net`]: what a host provides to send a [`Request`] and stream the answer back
//!   ([`Transport`]), and where keys are kept ([`Keys`]).
//!
//! Keys only go into a request's headers: [`Request`]'s `Debug` hides them, and provider
//! error text is scrubbed of them ([`net::scrub`]) before anything shows it.

pub mod chat;
pub mod net;
pub mod provider;
pub mod sse;
pub mod stream;

pub use chat::{Item, Stop, ToolCall, ToolResult, Turn, request};
pub use net::{Keys, MemoryKeys, Piece, Request, Stream, Transport};
pub use provider::{Kind, MODELS, Model, Settings, Usage};
pub use stream::{Decoder, Delta};
