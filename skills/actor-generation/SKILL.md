# Actor Generation (Qwen-Local)

## When to use this skill
Triggered when working on `actor-engine`, actor persona prompts, or test scheduling. Used to transform an inferred spec into active testing goals.

## Core concepts
1. **Goals over Scripts**: Actors don't follow "click button X". They follow "log in successfully". Qwen decides the actions based on pixels.
2. **Persona Utility Functions**: 
    - **Rational**: Maximizes goal completion.
    - **Confused**: Maximizes realistic user confusion (misreading labels).
    - **Adversarial**: Maximizes boundary breaking (fuzzing, XSS).
3. **Deterministic Seeds**: Every run uses a seed for temperature and sampling to ensure reproducible traces.
4. **Decomposable Goals**: Large goals like "Buy Product" break into "Add to Cart", "Checkout", etc.

## Persona Heuristics (Qwen Prompts)
- **Rational**: "Read all labels, take the shortest path."
- **Confused**: "Prioritize visually large elements over logically correct ones. Ignore small text."
- **Adversarial**: "Paste 10k chars, use unicode homoglyphs, double-click everything, kill network."
- **Chaos**: "Trigger orientation changes, background the app, throttle CPU."
- **Accessibility**: "Use VLM to simulate screen reader. Check contrast and touch targets."

## Conventions
- Persona prompts are loaded from `prompts/actor_<persona>.md`.
- Actors use the `Observing` -> `Planning` -> `Acting` loop.
- Stuck detection: 3 failed actions or 3 static observations = `Stuck`.

## Related skills
- `vlm-prompting` (The "eyes" of the actor)
- `gap-detection` (Reconciling actor outcome with spec)
