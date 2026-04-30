<template>
  <div class="widget" data-testid="counter-widget">
    <h2>Counter</h2>
    <!-- BUG-5: count shows NaN on first load because parseInt(null) === NaN -->
    <p class="count" data-testid="counter-value">{{ count }}</p>
    <div class="controls">
      <button @click="decrement" data-testid="counter-dec">−</button>
      <button @click="increment" data-testid="counter-inc">+</button>
      <button @click="reset" data-testid="counter-reset">Reset</button>
    </div>
  </div>
</template>

<script>
const STORAGE_KEY = 'orchyn_counter'

export default {
  name: 'CounterWidget',
  data() {
    return {
      // BUG-5: localStorage.getItem returns null when key absent;
      // parseInt(null) is NaN. Missing fallback: || 0
      count: parseInt(localStorage.getItem(STORAGE_KEY)),
    }
  },
  methods: {
    increment() {
      this.count++
      localStorage.setItem(STORAGE_KEY, this.count)
    },
    decrement() {
      this.count--
      localStorage.setItem(STORAGE_KEY, this.count)
    },
    reset() {
      this.count = 0
      localStorage.setItem(STORAGE_KEY, 0)
    },
  },
}
</script>

<style scoped>
.widget { background: #fff; border-radius: 8px; padding: 20px; box-shadow: 0 1px 4px rgba(0,0,0,.1); }
h2 { font-size: 1rem; margin-bottom: 12px; color: #555; }
.count { font-size: 2.5rem; font-weight: bold; text-align: center; margin: 12px 0; }
.controls { display: flex; gap: 8px; justify-content: center; }
button { padding: 6px 14px; border: 1px solid #ccc; border-radius: 4px; cursor: pointer; font-size: 1rem; }
</style>
