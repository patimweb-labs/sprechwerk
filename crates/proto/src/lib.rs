//! Sprache auf der Leitung.
pub mod frame;
pub mod nachricht;
pub use frame::{lese_rahmen, schreibe_rahmen, MAX_RAHMEN, PROTOKOLL_VERSION};
pub use nachricht::{
    ClientFrame, Fehlercode, Mitgliedsansicht, Raumansicht, ServerFrame, Sprachkopf, CODEC_OPUS,
    SPRACHKOPF_LEN,
};
