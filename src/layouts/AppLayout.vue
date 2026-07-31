<script setup lang="ts">
import type { ThemePreference } from "@/types/domain";

export type ViewKey = "space" | "moves" | "links" | "settings";

defineProps<{
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

const navigation: Array<{ key: ViewKey; label: string; description: string }> = [
  { key: "space", label: "空间分析", description: "扫描与迁移" },
  { key: "moves", label: "迁移记录", description: "检查与还原" },
  { key: "links", label: "链接检测", description: "直接子项" },
  { key: "settings", label: "设置", description: "托管位置" },
];
</script>

<template>
  <div class="app-layout">
    <aside class="sidebar">
      <div class="brand">
        <div class="brand-mark" aria-hidden="true">D</div>
        <div>
          <strong>DirMover</strong>
          <span>目录迁移工具</span>
        </div>
      </div>

      <nav aria-label="主导航">
        <button
          v-for="item in navigation"
          :key="item.key"
          type="button"
          :class="{ active: activeView === item.key }"
          :aria-current="activeView === item.key ? 'page' : undefined"
          @click="$emit('navigate', item.key)"
        >
          <strong>{{ item.label }}</strong>
          <span>{{ item.description }}</span>
        </button>
      </nav>

      <div class="sidebar-footer">
        <label>
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
        <span>Tauri 2 · Rust</span>
      </footer>
    </section>
  </div>
</template>

<style scoped>
.app-layout {
  display: grid;
  grid-template-columns: var(--sidebar-width) minmax(0, 1fr);
  width: 100vw;
  height: 100vh;
  color: var(--text-primary);
  background: var(--surface-app);
}

.sidebar {
  display: grid;
  grid-template-rows: auto 1fr auto;
  min-width: 0;
  padding: var(--space-3);
  border-right: 1px solid var(--border-subtle);
  background: var(--surface-sidebar);
}

.brand {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  height: 64px;
  padding-inline: var(--space-2);
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

.brand div:last-child {
  display: grid;
}

.brand strong {
  font-size: 15px;
  font-weight: 650;
}

.brand span {
  color: var(--text-secondary);
  font-size: 11px;
}

nav {
  display: grid;
  align-content: start;
  gap: 2px;
  padding-top: var(--space-3);
}

nav button {
  display: grid;
  gap: 1px;
  min-height: 48px;
  padding: 7px var(--space-3);
  border-left: 3px solid transparent;
  border-radius: var(--radius-small);
  color: var(--text-secondary);
  background: transparent;
  text-align: left;
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

nav span {
  font-size: 11px;
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
  min-width: 0;
  min-height: 0;
  grid-template-rows: minmax(0, 1fr) var(--status-height);
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
