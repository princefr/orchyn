# VLM Prompting (Qwen-Local)

## When to use this skill
Triggered when working on `pixel-observer` or the Qwen VLM prompt. Use when the model fails to see elements or hallucinates them.

## Core concepts
1. **Grounded Vision**: Qwen must provide a `bounding_box` for every element. If it can't point to it, it shouldn't report it.
2. **DP Coordinates**: The observer normalizes images to 1000x1000 (or platform DP) before calling Qwen.
3. **Structured UIState**: Qwen must return JSON. Freeform text is ignored.
4. **Anomalies Channel**: VLM is specifically asked to look for overflows, truncations, and contrast issues.
5. **Stateless Observation**: Each call is a single screenshot. No temporal memory in the VLM call itself.

## Anomaly Taxonomy
- `OverflowHorizontal/Vertical`: Content clipped by viewport.
- `Truncation`: Text ends in `...` unexpectedly.
- `ContrastLow`: Text/Background ratio < 4.5:1.
- `TouchTargetSmall`: Interactive area < 44x44 dp.
- `SafeAreaViolation`: Content under notch or status bar.
- `BrokenImage`: Placeholder visible.

## Common pitfalls
- **Hallucinating Values**: Qwen might "fill" an empty field. Prompt must ask to distinguish `placeholder` from `value`.
- **Z-Order Confusion**: Reporting elements obscured by a modal.
- **Keyboard Obscurity**: If `keyboard_visible` is true, mark bottom elements as `reachable: false`.

## Example Output
```json
{
  "elements": [
    {
      "kind": "Button",
      "label": "Login",
      "bounding_box": { "x": 100, "y": 200, "width": 80, "height": 40 },
      "enabled": true,
      "confidence": 0.95
    }
  ],
  "anomalies": [],
  "keyboard_visible": false
}
```

## Related skills
- `actor-generation`
- `gap-detection`
