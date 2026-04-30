<template>
  <div class="login-box">
    <h1>Sign In</h1>
    <form @submit.prevent="submit" data-testid="login-form">
      <div class="field">
        <label for="email">Email</label>
        <input
          id="email"
          v-model="email"
          type="text"
          placeholder="you@example.com"
          data-testid="email-input"
          autocomplete="username"
        />
      </div>
      <div class="field">
        <label for="password">Password</label>
        <input
          id="password"
          v-model="password"
          type="password"
          placeholder="Password"
          data-testid="password-input"
          autocomplete="current-password"
        />
      </div>
      <!-- BUG-2: no disabled state when fields are empty; submit fires with blank creds -->
      <button type="submit" data-testid="login-btn">Login</button>
      <p v-if="error" class="error" data-testid="login-error">{{ error }}</p>
    </form>
  </div>
</template>

<script>
export default {
  name: 'LoginForm',
  emits: ['login'],
  data() {
    return { email: '', password: '', error: '' }
  },
  methods: {
    submit() {
      this.error = ''
      // BUG-1 surface: accepts empty email — only checks password length
      if (!this.password) {
        this.error = 'Password is required'
        return
      }
      this.$emit('login', { email: this.email, password: this.password })
    },
  },
}
</script>

<style scoped>
.login-box { max-width: 360px; margin: 80px auto; background: #fff; padding: 32px; border-radius: 8px; box-shadow: 0 2px 8px rgba(0,0,0,.12); }
h1 { margin-bottom: 24px; font-size: 1.4rem; }
.field { margin-bottom: 16px; display: flex; flex-direction: column; gap: 4px; }
input { padding: 8px 12px; border: 1px solid #ccc; border-radius: 4px; font-size: 1rem; }
button { width: 100%; padding: 10px; background: #4f46e5; color: #fff; border: none; border-radius: 4px; font-size: 1rem; cursor: pointer; }
.error { color: #dc2626; margin-top: 12px; font-size: .9rem; }
</style>
