//! Invoice rendering — the headline use case.
//!
//! [`render_invoice`] takes an [`InvoiceData`] struct and
//! renders the built-in template ([`INVOICE_TEMPLATE`]) into
//! PDF bytes.
//!
//! The struct shape is deliberately decoupled from
//! `finance-proto::Invoice` so this crate doesn't depend on
//! finance. Callers (e.g. `finance::pdf::invoice_to_pdf`)
//! adapt the finance entity into [`InvoiceData`].

use serde::{Deserialize, Serialize};

use crate::error::PdfError;
use crate::render::render_template;

/// Wire shape for the invoice template. All money fields are
/// pre-formatted strings — the template doesn't do
/// arithmetic. Pass `"42.50"`, not `4250`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceData {
    /// Invoice number, e.g. `"INV-2026-0042"`.
    pub number: String,
    /// ISO 4217 currency code, e.g. `"USD"`. Drives the
    /// header symbol pick.
    pub currency: String,
    /// Slug for currency display — e.g. `"$"`, `"€"`. Empty
    /// = fall back to the code.
    pub currency_symbol: String,
    /// `YYYY-MM-DD`.
    pub issue_date: String,
    /// `YYYY-MM-DD`.
    pub due_date: String,
    /// One of `draft / sent / paid / void` (or whatever the
    /// caller's enum slug is).
    pub status: String,
    pub from: InvoiceParty,
    pub to: InvoiceParty,
    pub lines: Vec<InvoiceLine>,
    /// Pre-formatted strings, e.g. `"1,250.00"`.
    pub subtotal: String,
    pub tax: String,
    pub total: String,
    pub balance_due: String,
    pub notes: String,
    pub terms: String,
    pub footer: String,
}

/// Either side of the invoice header. Vendor on the left,
/// client on the right.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceParty {
    pub name: String,
    pub address: String,
    pub email: String,
    pub phone: String,
    /// VAT id / EIN / equivalent. Empty = hidden.
    pub tax_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceLine {
    pub description: String,
    /// e.g. `"4.50"` for hours, `"1"` for fixed-fee.
    pub quantity: String,
    /// e.g. `"hr"`, `"unit"`. Empty = hidden.
    pub unit: String,
    pub unit_price: String,
    pub amount: String,
}

/// Built-in invoice template. Embeds CSS Paged Media so the
/// line-items table paginates cleanly + a header repeats on
/// every page. Caller can opt out of the built-in by calling
/// [`crate::render_template`] with their own body.
pub const INVOICE_TEMPLATE: &str = include_str!("../templates/invoice.html");

/// Render an [`InvoiceData`] into PDF bytes via the built-in
/// template.
pub fn render_invoice(data: &InvoiceData) -> Result<Vec<u8>, PdfError> {
    render_template("invoice.html", INVOICE_TEMPLATE, data)
}
