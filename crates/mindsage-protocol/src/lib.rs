//! Privacy protocol — PII detection, anonymization, consent sessions.
//!
//! Provides regex-based PII detection covering structured PII types
//! (email, phone, SSN, credit card, IP, URL), token-based anonymization
//! with session-scoped de-anonymization, and consent session management.

pub mod consent;
pub mod pii;

pub use consent::{ConsentSession, ConsentManager};
pub use pii::{PiiDetector, PiiEntity, PiiType, AnonymizationResult};
