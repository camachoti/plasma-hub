export const appStorage = {
  get(key: string) {
    return localStorage.getItem(key);
  },

  set(key: string, value: string) {
    localStorage.setItem(key, value);
  },

  remove(key: string) {
    localStorage.removeItem(key);
  },

  getBoolean(key: string) {
    return localStorage.getItem(key) === "true";
  },

  setBoolean(key: string, value: boolean) {
    localStorage.setItem(key, value ? "true" : "false");
  },
};
