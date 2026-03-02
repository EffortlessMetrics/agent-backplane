# abp-shim-gemini

Drop-in Gemini SDK shim that routes through the Agent Backplane.

Provides a `GeminiClient` with an API surface that mirrors Google's
Gemini `generateContent` / `streamGenerateContent` endpoints. Internally,
requests are lowered to the ABP intermediate representation, converted to
`WorkOrder`s, and responses are projected back into native Gemini types.

## Usage

```rust,no_run
use abp_shim_gemini::{GeminiClient, GenerateContentRequest, Content, Part};

# async fn example() {
let client = GeminiClient::new("gemini-2.5-flash");
let request = GenerateContentRequest::new("gemini-2.5-flash")
    .add_content(Content::user(vec![Part::text("Hello!")]));
let response = client.generate(request).await.unwrap();
# }
```

## License

Licensed under MIT OR Apache-2.0.
