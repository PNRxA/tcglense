import { computed, ref } from "vue";
import { defineStore } from "pinia";
import type { LoginPayload, RegisterPayload, User } from "@/lib/api";
import { ApiError, login as apiLogin, me as apiMe, register as apiRegister } from "@/lib/api";

const TOKEN_KEY = "tcglense_token";

export const useAuthStore = defineStore("auth", () => {
  const token = ref<string | null>(localStorage.getItem(TOKEN_KEY));
  const user = ref<User | null>(null);

  const isAuthenticated = computed(() => Boolean(token.value));

  function setToken(value: string | null) {
    token.value = value;
    if (value) {
      localStorage.setItem(TOKEN_KEY, value);
    } else {
      localStorage.removeItem(TOKEN_KEY);
    }
  }

  async function login(payload: LoginPayload) {
    const response = await apiLogin(payload);
    setToken(response.token);
    user.value = response.user;
  }

  async function register(payload: RegisterPayload) {
    const response = await apiRegister(payload);
    setToken(response.token);
    user.value = response.user;
  }

  function logout() {
    setToken(null);
    user.value = null;
  }

  async function fetchMe() {
    if (!token.value) {
      return;
    }
    try {
      const response = await apiMe(token.value);
      user.value = response.user;
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) {
        logout();
      }
    }
  }

  return { token, user, isAuthenticated, login, register, logout, fetchMe };
});
