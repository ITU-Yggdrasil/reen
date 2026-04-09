## Application Entry-Point Specification

### Purpose
The application is a command-line tool named `retry` that retries a given command until it succeeds or a maximum number of attempts is reached. It is designed to wrap arbitrary shell commands and provide resilience by automatically retrying failed executions.

---

### Entry Point
The application is invoked via the command:
```
retry [OPTIONS] <COMMAND>...
```

---

### Parameters

#### Positional Arguments
1. **`<COMMAND>...`**
   - **Type**: List of strings (shell command and its arguments).
   - **Description**: The command to execute, including all arguments. The entire command is passed to the system shell for execution.
   - **Constraints**:
     - Must not be empty.
     - The command and its arguments are treated as a single unit for execution.
   - **Example**: `retry curl -X POST https://example.com/api`

#### Options
1. **`--attempts <ATTEMPTS>`**
   - **Type**: Integer.
   - **Description**: Maximum number of attempts to execute the command before giving up.
   - **Default**: `3` (if not specified).
   - **Constraints**:
     - Must be a positive integer (≥ 1).
   - **Example**: `retry --attempts 5 curl https://example.com`

2. **`--delay <DELAY>`**
   - **Type**: Duration string (e.g., `5s`, `100ms`, `1m`).
   - **Description**: Initial delay between retry attempts. The delay increases exponentially with each attempt (e.g., delay × 2, delay × 4, etc.).
   - **Default**: `1s` (if not specified).
   - **Constraints**:
     - Must be a positive duration (e.g., `1s`, `500ms`).
     - Supports common units: `ms` (milliseconds), `s` (seconds), `m` (minutes).
   - **Example**: `retry --delay 2s curl https://example.com`

3. **`--no-delay`**
   - **Type**: Flag.
   - **Description**: Disables the delay between retry attempts. If set, the command is retried immediately after a failure.
   - **Conflicts**: Cannot be used with `--delay`.
   - **Example**: `retry --no-delay curl https://example.com`

---

### Behavior

1. **Command Execution**
   - The provided `<COMMAND>...` is executed in the system shell.
   - The command is considered **successful** if it exits with a status code of `0`.
   - The command is considered **failed** if it exits with a non-zero status code.

2. **Retry Logic**
   - If the command succeeds on the first attempt, the application exits immediately with the same status code as the command.
   - If the command fails, the application waits for the specified delay (or no delay if `--no-delay` is set) and retries the command.
   - The delay between attempts increases exponentially (e.g., delay × 2, delay × 4, etc.) for each subsequent retry.
   - The retry logic continues until:
     - The command succeeds, or
     - The maximum number of attempts (`--attempts`) is reached.

3. **Termination**
   - If the command succeeds during any attempt, the application exits with the same status code as the command.
   - If the maximum number of attempts is reached and the command still fails, the application exits with the status code of the last failed attempt.

4. **Output**
   - The standard output (`stdout`) and standard error (`stderr`) of the executed command are streamed directly to the terminal in real-time.
   - No additional output (e.g., retry logs, delays) is produced by the `retry` tool itself.

---

### Environment Variables
No environment variables are explicitly referenced in the draft.

---

### Examples

1. **Basic Usage**
   ```
   retry curl https://example.com
   ```
   - Retries `curl https://example.com` up to 3 times with a 1-second delay between attempts.

2. **Custom Attempts and Delay**
   ```
   retry --attempts 5 --delay 2s curl https://example.com
   ```
   - Retries `curl https://example.com` up to 5 times with an initial delay of 2 seconds, doubling the delay after each attempt.

3. **No Delay**
   ```
   retry --no-delay curl https://example.com
   ```
   - Retries `curl https://example.com` up to 3 times with no delay between attempts.

---

### Blocking Ambiguities

1. **Exponential Delay Calculation**
   - The draft specifies that the delay increases exponentially but does not define the base of the exponent. For example:
     - Is the delay multiplied by 2 (e.g., `delay × 2`, `delay × 4`, `delay × 8`)?
     - Is the delay raised to a power (e.g., `delay^2`, `delay^3`)?
     - **Impact**: Affects the observable behavior of the application (e.g., how long it waits between retries).

2. **Shell Execution**
   - The draft states that the command is executed in the "system shell" but does not specify:
     - Which shell is used (e.g., `/bin/sh`, `/bin/bash`, or platform-specific defaults).
     - How the command is passed to the shell (e.g., as a single string or as an argument list).
   - **Impact**: Affects how commands are interpreted and executed, especially for complex commands (e.g., pipes, redirections, or shell built-ins).

3. **Handling of Signals**
   - The draft does not specify how the application handles signals (e.g., `SIGINT`, `SIGTERM`) sent to the `retry` process during command execution or delays.
   - **Impact**: Affects the observable behavior of the application when interrupted by the user or system.

4. **Exit Code Propagation**
   - The draft states that the application exits with the same status code as the command, but it does not specify:
     - Whether the exit code of the last failed attempt is propagated if the maximum number of attempts is reached.
     - How non-zero exit codes are handled if the command fails on all attempts.
   - **Clarification**: The draft implies that the exit code of the last attempt is used, but this is not explicitly stated.

---

### Implementation Choices Left Open

1. **Duration String Parsing**
   - The exact library or method for parsing duration strings (e.g., `5s`, `100ms`) is not specified.
   - **Non-blocking**: Any implementation that correctly parses positive durations with supported units is acceptable.

2. **Shell Invocation Mechanics**
   - The exact method for invoking the system shell (e.g., `std::process::Command` in Rust, `subprocess` in Python) is not specified.
   - **Non-blocking**: Any implementation that executes the command in the system shell is acceptable.

3. **Exponential Delay Implementation**
   - The exact mechanism for implementing the exponential delay (e.g., using a timer, sleep function, or async delays) is not specified.
   - **Non-blocking**: Any implementation that results in the correct delay behavior is acceptable.

4. **Output Streaming**
   - The exact method for streaming `stdout` and `stderr` of the executed command to the terminal is not specified.
   - **Non-blocking**: Any implementation that ensures real-time output streaming is acceptable.

5. **Error Handling for Invalid Inputs**
   - The draft does not specify how invalid inputs (e.g., negative `--attempts`, malformed `--delay`) are handled.
   - **Non-blocking**: The application may use any reasonable error-handling strategy (e.g., panics, error messages, or graceful exits) as long as it does not contradict the draft.