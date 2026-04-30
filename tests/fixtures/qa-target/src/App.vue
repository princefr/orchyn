<template>
  <div id="root">
    <LoginForm v-if="!user" @login="handleLogin" />
    <Dashboard v-else :user="user" @logout="handleLogout" />
  </div>
</template>

<script>
import LoginForm from './components/LoginForm.vue'
import Dashboard from './components/Dashboard.vue'

export default {
  name: 'App',
  components: { LoginForm, Dashboard },
  data() {
    return { user: null }
  },
  methods: {
    handleLogin(credentials) {
      // BUG-1: no email format validation — empty string is accepted
      if (credentials.password.length > 0) {
        this.user = {
          name: credentials.email.split('@')[0] || credentials.email,
          email: credentials.email,
        }
      }
    },
    handleLogout() {
      this.user = null
    },
  },
}
</script>
