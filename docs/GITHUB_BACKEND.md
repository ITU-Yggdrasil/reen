# GitHub Backend And MCP Setup

This guide explains how to:

1. store Reen drafts and specifications in GitHub issues
2. configure the official GitHub MCP server in an MCP-capable editor
3. create a fine-grained PAT with the minimum permissions Reen needs

## What `--github` Does

When you run Reen with:

```bash
reen --github <owner>/<repo> ...
```

Reen treats GitHub as the source of truth for drafts and specifications.

You can also set the backend in `reen.yml`:

```yaml
github: owner/repo
```

Precedence is:

1. `--github owner/repo`
2. `github: owner/repo` in `reen.yml`
3. local file mode using `drafts/` and `specifications/`

The current model is:

- draft artifacts: GitHub issues labeled `draft` plus one of `data`, `context`, `api`, or `app`
- specification artifacts: GitHub issues labeled `specification` plus one of `data`, `context`, `api`, or `app`
- specification -> draft link: stored in machine-readable metadata in the specification issue body
- direct dependencies: stored explicitly as GitHub sub-issues

Reen still materializes projected markdown files under `.reen/github/...` so the rest of the pipeline can keep using the existing implementation and test-generation flow.

## Projection Layout

When GitHub mode is enabled, Reen mirrors issue-backed artifacts under:

```text
.reen/github/<owner>__<repo>/
  drafts/
    app.md
    data/
    contexts/
    apis/
  specifications/
    app.md
    data/
    contexts/
    contexts/external/
```

That projected tree is what `check specification`, `create implementation`, and `create tests` consume.

## Recommended Authentication Model

There are two related integrations:

1. Reen CLI runtime
   Reen currently talks to GitHub through the `gh` CLI, so the machine running Reen must also have `gh` authenticated.
2. Editor MCP integration
   If you want AI tooling in your editor to use the official GitHub MCP server, configure the GitHub MCP server separately in the editor.

You can use the same fine-grained PAT for both.

## Fine-Grained PAT For Reen

### Token type

Create a fine-grained personal access token, not a classic PAT.

Recommended settings:

- Resource owner: the user or organization that owns the repo
- Repository access: `Only select repositories`
- Selected repositories: include the repo you will pass to `--github`
- Expiration: choose a short rotation window that fits your workflow

### Minimum repository permissions

For Reen's GitHub-backed draft/spec flow, the minimum useful repository permissions are:

- `Metadata: Read`
- `Issues: Read and write`

Why:

- Reen reads and writes issues
- Reen reads and writes labels on issues
- Reen reads and writes sub-issue relationships for explicit dependencies
- Reen needs repository metadata lookups to resolve the target repository cleanly

### Common optional permissions

Add these only if you want broader GitHub tooling in the same token:

- `Pull requests: Read and write`
  Useful for general GitHub MCP workflows in the editor, but not required for Reen's issue-backed artifact flow.
- `Contents: Read`
  Useful if you want editor MCP tools to inspect repository files.

### Organization caveats

If the target repo belongs to an organization:

- the org may require approval before the token can access the repo
- the org may restrict PAT usage entirely
- if you are an Enterprise Managed User, PAT auth may be disabled by policy

## Authenticate `gh` For Reen

Install the GitHub CLI first:

- macOS: `brew install gh`
- Windows: `winget install GitHub.cli`
- Linux: use your distro package manager or GitHub's install instructions

Authenticate with the PAT:

```bash
gh auth login --hostname github.com --with-token
```

Then paste the PAT on stdin.

Verify:

```bash
gh auth status
```

Once that works, Reen can use GitHub-backed artifacts:

```bash
reen --github owner/repo create specification
reen --github owner/repo check specification
reen --github owner/repo create implementation
reen --github owner/repo create tests
```

## Configure The Official GitHub MCP Server

GitHub's current docs recommend the hosted remote endpoint for most users:

```text
https://api.githubcopilot.com/mcp/
```

### Option 1: OAuth

This is the simplest option when your editor supports GitHub's MCP marketplace flow.

Use OAuth if:

- you do not need a manually managed token
- your org allows the GitHub OAuth flow
- you want the least setup friction

### Option 2: PAT

If your editor supports manual MCP config, add a GitHub server entry that points to the hosted endpoint and sends your PAT as a bearer token.

Example `mcp.json` snippet:

```json
{
  "servers": {
    "github": {
      "url": "https://api.githubcopilot.com/mcp/",
      "requestInit": {
        "headers": {
          "Authorization": "Bearer YOUR_GITHUB_PAT"
        }
      }
    }
  }
}
```

Replace `YOUR_GITHUB_PAT` with the fine-grained PAT you created.

If your editor distinguishes transport types, use the GitHub docs' recommended HTTP/SSE configuration for the hosted endpoint.

### VS Code / Cursor-style setup flow

If your editor exposes MCP server config through a UI, the usual values are:

- Server ID: `github`
- Type: `HTTP/SSE` or equivalent
- URL: `https://api.githubcopilot.com/mcp/`
- Header name: `Authorization`
- Header value: `Bearer <your fine-grained PAT>`

In VS Code specifically, GitHub's docs describe using the MCP marketplace and installing the GitHub MCP server from there.

## Optional: Local GitHub MCP Server

GitHub also documents a local/self-hosted GitHub MCP server path for users who want more control over deployment or network boundaries.

Use the local path if:

- you do not want to depend on the hosted MCP endpoint
- you need a self-hosted boundary for policy reasons
- you want to customize the local server deployment

The local deployment details can change, so use the official GitHub MCP server docs and repository for the exact current setup:

- [Setting up the GitHub MCP Server](https://docs.github.com/en/copilot/how-tos/provide-context/use-mcp/set-up-the-github-mcp-server)
- [Using the GitHub MCP Server](https://docs.github.com/en/copilot/how-tos/provide-context/use-mcp/use-the-github-mcp-server)
- [GitHub MCP server repository](https://github.com/github/github-mcp-server)

## Suggested Repo Conventions

For predictable Reen behavior, configure the target repository with these labels:

- `draft`
- `specification`
- `app`
- `data`
- `context`
- `api`

Recommended artifact mapping:

- drafts: one issue per draft
- specifications: one issue per generated specification
- `app.md`: use the `app` label
- direct dependencies: add the depended-on issue as a sub-issue

## Troubleshooting

### `gh auth status` fails

- re-run `gh auth login --with-token`
- confirm the token has not expired
- confirm the token includes the target repository

### Reen cannot see or update issues

- confirm the PAT has `Issues: Read and write`
- confirm the selected repository list includes the target repo
- if the repo is org-owned, check whether the token still needs org approval
- Reen currently projects only open issues; closed draft/specification issues are ignored until reopened

### Editor MCP setup works with OAuth but not with PAT

- verify the header is exactly `Authorization: Bearer <token>`
- confirm the PAT is fine-grained and scoped to the right repo
- confirm PAT auth is allowed by your organization or enterprise policy

### Reen works but the editor MCP server does not

- remember these are separate integrations
- `gh auth login` is required for Reen's current runtime path
- editor MCP auth must still be configured in the editor
