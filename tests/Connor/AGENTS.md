# Project Instructions

## Environment Variables

When implementing or changing configuration that uses environment variables:

- Create or update a `.env`-style template with sensible non-secret defaults.
- Keep the variable names in the template and in code exactly aligned.
- Load configuration from `.env` in code, while preserving the usual precedence: real process environment variables override values from `.env`.
- Prefer defaults in `.env` for local development; reserve truly required variables for values that cannot have a safe default.
- Never introduce real secrets into committed templates or example files.

For this repository specifically:

- Use `.env.example` as the committed template.
- If runtime code reads environment variables, it must load `.env` before reading them.
