<template>
  <div class="widget" data-testid="todo-widget">
    <h2>Todos</h2>
    <ul data-testid="todo-list">
      <li
        v-for="(todo, i) in todos"
        :key="i"
        class="todo-item"
        :data-testid="`todo-item-${i}`"
      >
        <span>{{ todo.text }}</span>
        <div class="actions">
          <input
            type="checkbox"
            :checked="todo.done"
            @change="toggle(i)"
            :data-testid="`todo-check-${i}`"
          />
          <!-- BUG-6: passes i+1 instead of i — off-by-one removes the wrong item -->
          <button @click="remove(i + 1)" :data-testid="`todo-delete-${i}`">✕</button>
        </div>
      </li>
    </ul>
    <div class="add-row">
      <!-- BUG-7: input is NOT cleared after adding — newTodo retains old value -->
      <input
        v-model="newTodo"
        placeholder="What needs doing?"
        data-testid="todo-input"
        @keyup.enter="add"
      />
      <button @click="add" data-testid="todo-add-btn">Add</button>
    </div>
    <p v-if="todos.length === 0" class="empty" data-testid="todo-empty">
      No todos yet. Add one above!
    </p>
  </div>
</template>

<script>
export default {
  name: 'TodoList',
  data() {
    return {
      todos: [
        { text: 'Write tests', done: false },
        { text: 'Fix bugs', done: false },
      ],
      newTodo: '',
    }
  },
  methods: {
    add() {
      if (this.newTodo.trim()) {
        this.todos.push({ text: this.newTodo.trim(), done: false })
        // BUG-7: missing → this.newTodo = ''
      }
    },
    toggle(i) {
      this.todos[i].done = !this.todos[i].done
    },
    remove(i) {
      // BUG-6: called with i+1 from template → removes wrong item; when i=last, splice removes nothing
      this.todos.splice(i, 1)
    },
  },
}
</script>

<style scoped>
.widget { background: #fff; border-radius: 8px; padding: 20px; box-shadow: 0 1px 4px rgba(0,0,0,.1); }
h2 { font-size: 1rem; margin-bottom: 12px; color: #555; }
ul { list-style: none; margin-bottom: 12px; }
.todo-item { display: flex; justify-content: space-between; align-items: center; padding: 6px 0; border-bottom: 1px solid #f0f0f0; }
.actions { display: flex; align-items: center; gap: 8px; }
.add-row { display: flex; gap: 8px; }
input[type="text"], .add-row input { flex: 1; padding: 6px 10px; border: 1px solid #ccc; border-radius: 4px; }
button { padding: 6px 12px; border: 1px solid #ccc; border-radius: 4px; cursor: pointer; }
.empty { color: #999; font-size: .9rem; text-align: center; padding: 12px 0; }
</style>
