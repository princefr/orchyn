<template>
  <div class="dashboard">
    <header>
      <h1 data-testid="welcome-heading">Welcome, {{ user.name }}!</h1>
      <button @click="$emit('logout')" data-testid="logout-btn">Log out</button>
    </header>

    <section class="widgets">
      <CounterWidget />
      <TodoList />
    </section>

    <section class="danger-zone">
      <!-- BUG-3: triggers console.error AND calls undefined method → TypeError in console -->
      <button @click="triggerError" data-testid="error-btn" class="danger">
        Trigger Error
      </button>
      <!-- BUG-4: export button calls a function that does not exist -->
      <button @click="exportData" data-testid="export-btn">Export</button>
    </section>
  </div>
</template>

<script>
import CounterWidget from './CounterWidget.vue'
import TodoList from './TodoList.vue'

export default {
  name: 'Dashboard',
  components: { CounterWidget, TodoList },
  props: { user: Object },
  emits: ['logout'],
  methods: {
    triggerError() {
      console.error('intentional_test_error: should not reach production')
      this.nonExistentMethod()  // BUG-3: TypeError — method does not exist
    },
    // BUG-4: exportData is referenced in the template but never defined here
  },
}
</script>

<style scoped>
.dashboard { max-width: 800px; margin: 0 auto; }
header { display: flex; justify-content: space-between; align-items: center; padding: 16px 0 24px; border-bottom: 1px solid #e5e5e5; margin-bottom: 24px; }
h1 { font-size: 1.5rem; }
.widgets { display: grid; grid-template-columns: 1fr 1fr; gap: 24px; }
.danger-zone { margin-top: 32px; display: flex; gap: 12px; }
button { padding: 8px 16px; border: 1px solid #ccc; border-radius: 4px; cursor: pointer; background: #fff; }
.danger { border-color: #dc2626; color: #dc2626; }
</style>
