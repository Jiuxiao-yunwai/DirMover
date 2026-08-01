<script setup lang="ts">
import { ref, watch } from "vue";
import NavIcon from "@/components/base/NavIcon.vue";
import type { BootstrapData, ThemePreference } from "@/types/domain";

export type ViewKey = "space" | "moves" | "links" | "settings" | "help";
type NavIconName = "space" | "moves" | "links" | "settings" | "help";

defineProps<{
  bootstrap: BootstrapData;
  activeView: ViewKey;
  isAdministrator: boolean;
  theme: ThemePreference;
  status: string;
  busy: boolean;
}>();

defineEmits<{
  navigate: [view: ViewKey];
  "update:theme": [theme: ThemePreference];
}>();

const navigation: Array<{ key: ViewKey; label: string; icon: NavIconName }> = [
  { key: "space", label: "空间分析", icon: "space" },
  { key: "moves", label: "迁移记录", icon: "moves" },
  { key: "links", label: "链接检测", icon: "links" },
  { key: "settings", label: "设置", icon: "settings" },
  { key: "help", label: "帮助", icon: "help" },
];

const sidebarStorageKey = "dirmover.sidebar.collapsed";
const collapsed = ref(localStorage.getItem(sidebarStorageKey) === "true");

watch(collapsed, (value) => {
  localStorage.setItem(sidebarStorageKey, String(value));
});
</script>

<template>
  <div class="app-layout" :class="{ collapsed }">
    <aside class="sidebar">
      <div class="brand">
        <button
          class="brand-logo"
          type="button"
          :disabled="!collapsed"
          :title="collapsed ? '展开菜单' : undefined"
          :aria-label="collapsed ? '展开菜单' : 'DirMover'"
          @click="collapsed = false"
        >
          <span class="brand-mark" aria-hidden="true">D</span>
          <span class="expand-chevron" aria-hidden="true">
            <svg viewBox="0 0 10 16" fill="none">
              <path d="m3 3 5 5-5 5" />
            </svg>
          </span>
        </button>
        <div class="brand-copy">
          <strong>DirMover</strong>
          <span class="version-label">v{{ bootstrap.version }}</span>
        </div>
        <button
          class="collapse-toggle"
          type="button"
          :title="collapsed ? '展开菜单' : '折叠菜单'"
          :aria-label="collapsed ? '展开菜单' : '折叠菜单'"
          @click="collapsed = !collapsed"
        >
          <span aria-hidden="true" />
          <span aria-hidden="true" />
          <span aria-hidden="true" />
        </button>
      </div>

      <nav aria-label="主导航">
        <button
          v-for="item in navigation"
          :key="item.key"
          type="button"
          :title="collapsed ? item.label : undefined"
          :class="{ active: activeView === item.key }"
          :aria-current="activeView === item.key ? 'page' : undefined"
          @click="$emit('navigate', item.key)"
        >
          <span class="nav-icon" aria-hidden="true">
            <NavIcon :name="item.icon" />
          </span>
          <strong class="nav-label">{{ item.label }}</strong>
        </button>
      </nav>

      <div class="sidebar-footer">
        <label class="theme-control">
          <span>主题</span>
          <select
            :value="theme"
            @change="
              $emit(
                'update:theme',
                ($event.target as HTMLSelectElement).value as ThemePreference,
              )
            "
          >
            <option value="system">跟随系统</option>
            <option value="light">浅色</option>
            <option value="dark">深色</option>
          </select>
        </label>
        <div class="engine-state">
          <span class="state-dot" :class="{ admin: isAdministrator }" />
          {{ isAdministrator ? "管理员模式 · MFT 可用" : "普通模式 · 扫描时提权" }}
        </div>
      </div>
    </aside>

    <section class="workspace">
      <main>
        <slot />
      </main>
      <footer class="statusbar">
        <span class="status-dot" :class="{ busy }" />
        <span>{{ status }}</span>
        <span class="status-spacer" />
        <span>v{{ bootstrap.version }} · Tauri 2 · Rust</span>
      </footer>
    </section>
  </div>
</template>

<style scoped>
.app-layout {
  display: flex;
  width: 100vw;
  height: 100vh;
  overflow: hidden;
  color: var(--text-primary);
  background: var(--surface-app);
}

.sidebar {
  display: grid;
  width: var(--sidebar-width);
  flex: 0 0 auto;
  grid-template-rows: auto 1fr auto;
  min-width: 0;
  overflow: hidden;
  padding: var(--space-3);
  border-right: 1px solid var(--border-subtle);
  background: var(--surface-sidebar);
  contain: layout paint;
}

.collapsed .sidebar {
  width: 72px;
}

.brand {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  height: 64px;
  padding-inline: var(--space-2);
  white-space: nowrap;
}

.collapsed .brand {
  gap: 0;
  padding-inline: 0;
}

.brand-logo {
  position: relative;
  display: flex;
  width: 32px;
  height: 32px;
  flex: 0 0 32px;
  align-items: center;
  padding: 0;
  overflow: visible;
  color: inherit;
  background: transparent;
}

.brand-logo:disabled {
  cursor: default;
  opacity: 1;
}

.collapsed .brand-logo {
  width: 48px;
  flex-basis: 48px;
  cursor: pointer;
}

.collapsed .brand-logo .brand-mark {
  margin-left: 8px;
}

.brand-mark {
  display: grid;
  width: 32px;
  height: 32px;
  place-items: center;
  border-radius: var(--radius-medium);
  color: #fff;
  background: var(--accent);
  font-size: 16px;
  font-weight: 700;
}

.brand-copy {
  display: grid;
  min-width: 0;
  flex: 1;
  overflow: hidden;
  opacity: 1;
  transform: translateX(0);
}

.brand strong {
  font-size: 15px;
  font-weight: 650;
}

.version-label {
  color: var(--text-disabled);
  font-size: 11px;
  white-space: nowrap;
}

.collapsed .theme-control,
.collapsed .engine-state {
  display: none;
}

.collapsed .brand-copy {
  flex: 0 0 0;
  opacity: 0;
  transform: translateX(-6px);
}

.expand-chevron {
  position: absolute;
  left: 39px;
  display: grid;
  width: 9px;
  height: 16px;
  place-items: center;
  color: var(--text-secondary);
  opacity: 0;
  transform: translateX(-4px);
  transition:
    opacity var(--duration-fast) var(--ease-standard),
    transform var(--duration-fast) var(--ease-standard);
}

.expand-chevron svg {
  width: 9px;
  height: 16px;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.collapsed .brand-logo:hover .expand-chevron,
.collapsed .brand-logo:focus-visible .expand-chevron {
  opacity: 1;
  transform: translateX(0);
}

nav {
  display: grid;
  align-content: start;
  gap: 2px;
  padding-top: var(--space-3);
}

nav button {
  display: grid;
  grid-template-columns: 20px minmax(0, 1fr);
  min-height: 42px;
  align-items: center;
  gap: var(--space-3);
  overflow: hidden;
  padding: 7px var(--space-3);
  border-left: 3px solid transparent;
  border-radius: var(--radius-small);
  color: var(--text-secondary);
  background: transparent;
  text-align: left;
  white-space: nowrap;
  transition:
    background var(--duration-fast) var(--ease-standard),
    color var(--duration-fast) var(--ease-standard);
}

.collapsed nav button {
  grid-template-columns: 20px 0;
  justify-content: center;
  gap: 0;
  padding: 7px 0;
}

nav button:hover {
  background: var(--surface-hover);
}

nav button.active {
  border-left-color: var(--accent);
  color: var(--text-primary);
  background: var(--surface-selected);
}

nav strong {
  font-size: 13px;
  font-weight: 600;
}

.nav-icon {
  display: grid;
  width: 20px;
  flex: 0 0 20px;
  place-items: center;
  color: var(--text-secondary);
  transition: color var(--duration-fast) var(--ease-standard);
}

nav button.active .nav-icon {
  color: var(--accent);
}

.nav-label {
  min-width: 0;
  overflow: hidden;
  opacity: 1;
  transform: translateX(0);
}

.collapsed .nav-label {
  opacity: 0;
  transform: translateX(-5px);
}

.sidebar-footer {
  display: grid;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-2) var(--space-1);
  border-top: 1px solid var(--border-subtle);
}

.sidebar-footer label {
  display: grid;
  grid-template-columns: 1fr auto;
  align-items: center;
  color: var(--text-secondary);
  font-size: var(--font-size-small);
}

select {
  height: var(--control-height-small);
  padding: 0 26px 0 var(--space-2);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-small);
  color: var(--text-primary);
  background: var(--surface-primary);
}

.engine-state {
  color: var(--text-secondary);
  font-size: 11px;
}

.collapse-toggle {
  display: grid;
  width: 28px;
  height: 28px;
  flex: 0 0 28px;
  align-content: center;
  gap: 3px;
  padding: 6px;
  border-radius: var(--radius-small);
  color: var(--text-secondary);
  background: transparent;
  opacity: 1;
  transform: scale(1);
  transition: background var(--duration-fast) var(--ease-standard);
}

.collapse-toggle:hover {
  color: var(--text-primary);
  background: var(--surface-hover);
}

.collapse-toggle span {
  display: block;
  width: 16px;
  height: 1.5px;
  border-radius: 1px;
  background: currentColor;
}

.collapsed .collapse-toggle {
  visibility: hidden;
  width: 0;
  flex-basis: 0;
  padding: 0;
  opacity: 0;
  pointer-events: none;
  transform: scale(0.75);
}

.collapsed .sidebar-footer {
  display: none;
}

.state-dot,
.status-dot {
  display: inline-block;
  width: 7px;
  height: 7px;
  margin-right: var(--space-2);
  border-radius: 50%;
  background: var(--warning);
}

.state-dot.admin {
  background: var(--success);
}

.workspace {
  display: grid;
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  grid-template-rows: minmax(0, 1fr) var(--status-height);
}

@media (prefers-reduced-motion: reduce) {
  .sidebar,
  .brand-copy,
  .expand-chevron,
  nav button,
  .nav-icon,
  .nav-label,
  .collapse-toggle {
    transition: none;
  }
}

main {
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.statusbar {
  display: flex;
  align-items: center;
  padding: 0 var(--space-4);
  border-top: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  background: var(--surface-primary);
  font-size: 11px;
}

.status-dot {
  background: var(--success);
}

.status-dot.busy {
  background: var(--accent);
  animation: pulse 1.2s ease-in-out infinite;
}

.status-spacer {
  flex: 1;
}

@keyframes pulse {
  50% {
    opacity: 0.35;
  }
}
</style>
