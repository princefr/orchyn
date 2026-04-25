# Platform Adapters (Qwen-Local)

## When to use this skill
Triggered when adding or modifying a platform crate (`platform-web`, `platform-android`, etc.). Use when translating abstract actor actions into native gestures.

## Core concepts
1. **The Contract**: Implement `PlatformAdapter`. Actors call `screenshot()`, `execute()`, `reset()`.
2. **DP vs Native**: Actors plan in DP (1000x1000 or device DP). Adapters project DP to native pixels using `DeviceInfo`.
3. **Bulletproof Reset**: `reset()` must clear ALL state (storage, cookies, modals).
4. **Session Isolation**: One adapter instance = One session. No sharing devices.

## Platform Specifics
- **Web**: Playwright via JSON-RPC. Use fresh `BrowserContext`.
- **Android**: ADB shell commands. Avoid `/sdcard` for screenshots (use `exec-out`).
- **iOS**: Appium/WebDriverAgent. Use W3C Actions for complex gestures.
- **Flutter**: Integration Test driver. Use VM Service protocol for high-fidelity screenshots.

## Common pitfalls
- **Serial Collisions**: Always pass `-s <serial>` for ADB.
- **DPI Rounding**: Round DP to nearest integer native pixels.
- **Keyboard issues**: ADB `input text` is slow and can fail on special chars. Use `broadcast` or `content` providers for faster input if possible.

## Example Implementation
```rust
impl PlatformAdapter for AndroidAdapter {
    async fn screenshot(&mut self) -> Result<Vec<u8>> {
        // adb exec-out screencap -p
    }
    async fn execute(&mut self, action: &Action) -> Result<()> {
        // project DP to native and adb shell input tap x y
    }
}
```

## Related skills
- `vlm-prompting` (DeviceInfo feeds normalization)
- `actor-generation` (Actors call the adapter)
