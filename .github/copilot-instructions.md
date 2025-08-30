# AI Assistant Guidelines for ESP32-C6 Embassy Charger Project

## Project Context

- **Hardware**: XIAO ESP32-C6 microcontroller with 64KB heap allocation
- **Framework**: esp-hal and Embassy async runtime for embedded Rust (no_std)
- **Purpose**: OCPP 1.6 compliant electric vehicle charging station Proof of Concept
- **Protocols**:
  - **OCPP**: Open Charge Point Protocol for EV charging communication
  - **MQTT**: Message Queuing Telemetry Transport for cloud connectivity
- **Communication**: MQTT over WiFi for cloud connectivity
- **Memory**: Constrained embedded environment requiring careful resource management

## Core Behavioral Principles

### 1. Always Verify Compilation

- **MUST** run `cargo check` after any code modifications
- **MUST** resolve all compilation errors before concluding
- **MUST** run `cargo fmt --all` and `cargo clippy` when finished with a task
- **SHOULD** follow clippy suggestions
- **SHOULD** address warnings when practical
- **EXPLAIN** any remaining warnings if they cannot be resolved

### 2. Follow Embassy Async Patterns

- **USE** Embassy tasks (`#[embassy_executor::task]`) for concurrent operations
- **USE** Embassy-sync primitives (Channel, Mutex, Signal) for task communication
- **AVOID** std library - use core and Embassy equivalents
- **PREFER** async/await patterns over blocking operations

### 3. Memory Management Best Practices

- **USE** heapless collections (Vec, String) instead of std equivalents
- **AVOID** large allocations or dynamic memory where possible
- **CHECK** buffer sizes match expected data (MQTT messages, OCPP payloads)
- **CONSIDER** memory constraints when sizing buffers or collections

### 4. Network Communication Patterns

- **USE** the MQTT message queue pattern to prevent deadlocks
- **SERIALIZE** access to network resources using mutexes or channels
- **INCLUDE** proper error handling for network operations
- **ADD** informative logging for network events (connect, send, receive, errors)

### 5. Code Quality Standards

- **FOLLOW** existing code patterns and naming conventions
- **ADD** descriptive comments for complex logic only
- **USE** `info!()` for important events, `warn!()` for recoverable errors
- **HANDLE** errors gracefully with proper Result types
- **PREFER** small, focused functions over large monolithic ones

### 6. Sourcecode and version control

- **MUST** when asked to create a new feature, by the prompt using the words "new feature", create a feature branch suggesting a name from the text prefixed with `vibe_`
- **MUST** do not add or commit code to git yourself

## Technical Decision Framework

### When Making Changes:

1. **EXPLAIN** the reasoning behind technical decisions
2. **IDENTIFY** potential trade-offs or alternatives
3. **CONSIDER** impact on memory usage and performance
4. **ENSURE** changes fit with existing architecture patterns
5. **VALIDATE** that Embassy async patterns are maintained

## Communication Style

### Code Explanations:

- **START** with a brief summary of what will be changed
- **EXPLAIN** why the change is necessary
- **DESCRIBE** how the implementation works
- **HIGHLIGHT** any important considerations or caveats

### Error Handling:

- **PROVIDE** specific error messages and debugging context
- **SUGGEST** potential solutions when compilation fails
- **EXPLAIN** the root cause of issues when possible

### Documentation:

- **USE** structured formatting (bullet points, numbered lists)
- **INCLUDE** code examples for complex concepts
- **REFERENCE** relevant Embassy or Rust documentation when helpful

## Project-Specific Patterns


### Error Logging Pattern:
  - When logging with string interpolation, put the variables inside the {} in the string instead of arguments

```rust
match operation().await {
    Ok(result) => info!("Operation successful: {result}"),
    Err(e) => warn!("Operation failed: {:?}", e),
}
```

## When In Doubt

### Ask for Clarification:

- If requirements are ambiguous or could be interpreted multiple ways
- When multiple implementation approaches have significant trade-offs
- If proposed changes might affect system stability or performance

### Reference Existing Code:

- Look for similar patterns already implemented in the codebase
- Follow established conventions for naming, error handling, and structure
- Maintain consistency with existing Embassy task patterns

### Suggest Alternatives:

- Present multiple approaches with pros/cons when appropriate
- Explain trade-offs between memory usage, performance, and code complexity
- Consider both immediate implementation and future maintainability

## Success Criteria

A successful interaction should result in:

- ✅ Code that compiles without errors
- ✅ Follows Embassy async patterns correctly
- ✅ Maintains memory efficiency appropriate for embedded systems
- ✅ Includes proper error handling and logging
- ✅ Is well-documented and follows project conventions
- ✅ Integrates seamlessly with existing architecture

## Example Interaction Pattern

1. **Understand** the request and current code context
2. **Explain** what changes will be made and why
3. **Implement** changes following these guidelines
4. **Verify** compilation with `cargo check`
5. **Summarize** what was accomplished and any important notes
