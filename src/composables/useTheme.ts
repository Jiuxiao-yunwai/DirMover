import { onBeforeUnmount, ref, watch } from "vue";
import type { ThemePreference } from "@/types/domain";

const STORAGE_KEY = "dirmover.theme";

export function useTheme() {
  const stored = localStorage.getItem(STORAGE_KEY);
  const preference = ref<ThemePreference>(
    stored === "light" || stored === "dark" ? stored : "system",
  );
  const media = window.matchMedia("(prefers-color-scheme: dark)");

  const apply = (): void => {
    const resolved =
      preference.value === "system"
        ? media.matches
          ? "dark"
          : "light"
        : preference.value;
    document.documentElement.dataset.theme = resolved;
    document.documentElement.style.colorScheme = resolved;
  };

  const onSystemThemeChanged = (): void => {
    if (preference.value === "system") {
      apply();
    }
  };

  media.addEventListener("change", onSystemThemeChanged);
  watch(
    preference,
    (value) => {
      localStorage.setItem(STORAGE_KEY, value);
      apply();
    },
    { immediate: true },
  );
  onBeforeUnmount(() => {
    media.removeEventListener("change", onSystemThemeChanged);
  });

  return { preference };
}
