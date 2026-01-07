# Incremental Build System

Reen includes an intelligent incremental build system that tracks file changes and only regenerates what's necessary.

## Overview

The incremental build system uses file hashing to detect changes and maintains a dependency graph between build stages. This dramatically speeds up development by avoiding unnecessary LLM API calls.

## How It Works

### Build Tracker

All build information is stored in the `.reen/` directory:
```
.reen/
└── build_tracker.json    # Stores file hashes and dependencies
```

### Tracked Stages

Reen tracks dependencies between these stages:

```
drafts/
   ↓ (Stage::Specification)
contexts/
   ↓ (Stage::Implementation)
src/contexts/
   ↓ (Stage::Compile)
target/
```

### Hash-Based Change Detection

For each transformation, reen stores:
- **Input hash**: SHA256 hash of the input file
- **Output hash**: SHA256 hash of the output file
- **Timestamp**: When the transformation occurred

## Dependency Rules

### Specification Stage
- **Input**: `drafts/*.md`
- **Output**: `contexts/*.md`
- **Upstream**: None
- **Regenerates when**: Draft file changes

### Implementation Stage
- **Input**: `contexts/*.md`
- **Output**: `src/contexts/*.rs`
- **Upstream**: Specification
- **Regenerates when**:
  - Context file changes
  - Corresponding draft was updated (specification needs regeneration)

### Tests Stage
- **Input**: `contexts/*.md`
- **Output**: `tests/*.rs`
- **Upstream**: Specification
- **Regenerates when**: Context file changes

### Compile/Run/Test Stages
- **Input**: `src/contexts/*.rs`
- **Output**: `target/`
- **Upstream**: Implementation
- **Regenerates when**: Any source file changes

## Usage

### Automatic Incremental Builds

By default, all commands use incremental builds:

```bash
# First run - generates everything
reen create specification

# Second run - skips unchanged files
reen create specification
# Output: "All specifications are up to date"

# After modifying a draft
reen create specification
# Only regenerates the changed file
```

### Verbose Output

See what's being skipped:

```bash
reen --verbose create specification
```

Output:
```
⊚ Skipping file_cache (up to date)
⊚ Skipping agent_runner (up to date)
Processing draft: app
✓ Successfully created specification for app
```

### Forcing Regeneration

To force regeneration of everything:

```bash
# Delete the build tracker
rm -rf .reen/

# Or edit the file to trigger regeneration
touch drafts/app.md
```

## Examples

### Typical Development Workflow

```bash
# 1. Initial generation
reen create specification     # Processes all drafts
reen create implementation    # Processes all contexts
reen compile                  # Builds everything

# 2. Edit a single draft
vim drafts/account.md

# 3. Regenerate (only account spec is regenerated)
reen create specification     # Fast! Only processes account.md

# 4. Implementation detects upstream change
reen create implementation    # Only regenerates account.rs

# 5. Compile
reen compile                  # Only recompiles changed files
```

### Checking Build Status

The build tracker stores metadata about each file:

```json
{
  "tracks": {
    "Specification": {
      "account": {
        "input_hash": "a1b2c3...",
        "output_hash": "d4e5f6...",
        "timestamp": "2024-01-15T10:30:00Z"
      }
    }
  }
}
```

## Benefits

### 1. **Speed**
- Skip unnecessary LLM API calls
- Only regenerate what changed
- Faster iteration during development

### 2. **Cost Savings**
- Fewer API calls = lower costs
- Especially important for large codebases

### 3. **Predictability**
- Clear dependency chain
- Always know what will be regenerated
- Reproducible builds

### 4. **Safety**
- Don't accidentally overwrite manual changes
- Track what was generated vs what was edited

## Advanced Usage

### Viewing Tracker Status

```bash
# View the raw tracker
cat .reen/build_tracker.json | jq
```

### Manual Tracker Management

```bash
# Reset all tracking (force full rebuild)
rm -rf .reen/

# Reset only specifications
rm .reen/build_tracker.json
# Then edit it to remove the "Specification" key
```

### Understanding Skips

When reen skips a file, it means:
1. The output file exists
2. The input file hasn't changed (hash matches)
3. No upstream dependencies have changed

## Limitations

### Current Limitations

1. **Single-file tracking**: Each file is tracked independently
   - If implementation depends on multiple specs, only direct dependency is checked

2. **No cross-file dependencies**: Changes in one context don't trigger regeneration of dependent contexts
   - Example: If `MoneyTransfer` uses `Account`, changing `Account` doesn't auto-regenerate `MoneyTransfer`

3. **Manual edits not detected**: If you manually edit generated files, reen won't know
   - The output hash will change, but reen uses input hash for skip decisions

### Future Enhancements

Planned improvements:
- Dependency graph between contexts
- Detect manual edits to generated files
- Incremental agent execution (partial regeneration)
- Build cache across branches

## Troubleshooting

### "Up to date" but output is wrong

If reen says files are up to date but the output is wrong:

```bash
# Force regeneration
rm -rf .reen/
reen create specification
```

### Changes not being detected

Check if you're editing the right file:
- Edit `drafts/*.md` not `contexts/*.md` for specifications
- Edit `contexts/*.md` not `src/contexts/*.rs` for implementation

### Tracker corruption

If the tracker gets corrupted:

```bash
rm -rf .reen/
# Start fresh - all files will regenerate
```

## Best Practices

1. **Don't edit generated files directly**
   - Edit the source (drafts/contexts) instead
   - Let reen regenerate the output

2. **Commit .reen/ to git** (optional)
   - Team shares incremental build state
   - CI can skip unchanged files
   - However, hashes are environment-specific

3. **Use verbose mode during development**
   - See what's being skipped
   - Understand the build process
   - Debug issues

4. **Clean builds for releases**
   - `rm -rf .reen/ && reen create specification && ...`
   - Ensures everything is fresh

## Integration with CI/CD

### GitHub Actions Example

```yaml
- name: Cache reen build tracker
  uses: actions/cache@v3
  with:
    path: .reen/
    key: ${{ runner.os }}-reen-${{ hashFiles('drafts/**') }}

- name: Generate code
  run: |
    reen create specification
    reen create implementation
```

This caches the build tracker across CI runs, speeding up builds.

## Summary

The incremental build system makes reen practical for real-world development by:

- ✅ Tracking file changes with SHA256 hashes
- ✅ Maintaining dependency graph between stages
- ✅ Skipping unnecessary regeneration
- ✅ Saving time and API costs
- ✅ Providing clear feedback on what's happening

All stored in a simple `.reen/build_tracker.json` file that you can inspect, edit, or delete as needed.
