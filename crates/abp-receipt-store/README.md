# abp-receipt-store

Persistent receipt storage and querying for the Agent Backplane.

Provides a `ReceiptStore` async trait with two implementations:

- **`InMemoryReceiptStore`** — fast, HashMap-backed, for testing and ephemeral use.
- **`FileReceiptStore`** — JSON-lines file-based, for durable persistence.

Also includes `ReceiptIndex` for fast in-memory lookup by backend, outcome,
and time range, plus `validate_chain` for receipt chain integrity verification.

## License

MIT OR Apache-2.0
