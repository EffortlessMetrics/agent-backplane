# Codegen

Backplane treats **Rust as the spec**:

- `abp-core` defines the contract types.
- `xtask schema` produces JSON Schemas.
- Other languages (TypeScript, Python, Go, etc.) should be generated from the schemas.

This repo does not pin a specific generator yet.

## Suggested generators

- TypeScript: `quicktype` (JSON Schema -> TS)
- Python: `datamodel-code-generator` (JSON Schema -> Pydantic)

## Example flow (manual)

```bash
# 1) Generate schemas
cargo run -p xtask -- schema

# 2) TypeScript types (example)
npx quicktype \
  --src contracts/schemas/work_order.schema.json \
  --src contracts/schemas/receipt.schema.json \
  --lang ts \
  --out codegen/out/abp_types.ts

# 3) Python types (example)
python -m pip install datamodel-code-generator
python -m datamodel_code_generator \
  --input contracts/schemas/work_order.schema.json \
  --input-file-type jsonschema \
  --output codegen/out/work_order.py
```

The key requirement is that generated types remain compatible with the JSON produced by Rust.

