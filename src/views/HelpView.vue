<template>
  <section class="view">
    <header class="page-header">
      <h1>帮助</h1>
    </header>

    <div class="help-content">
      <section class="quick-start">
        <h2>快速开始</h2>
        <ol>
          <li>在“空间分析”选择常用位置，或点击路径右侧的文件夹按钮。</li>
          <li>应用会立即列出一级文件夹；扫描大小是可选操作，不会阻止其他功能。</li>
          <li>选择任意普通文件夹后即可规划迁移，迁移引擎会独立完成测量和安全校验。</li>
        </ol>
      </section>

      <div class="module-grid">
        <section>
          <h2>空间分析</h2>
          <ul>
            <li>“扫描”读取目录大小；管理员模式优先使用 NTFS MFT。</li>
            <li>点击表头排序，磁盘占用底色表示该目录占扫描总量的比例。</li>
            <li>双击目录可打开位置；右键可以打开或复制完整路径。</li>
            <li>扫描期间可以切换到其他模块，任务和结果不会丢失。</li>
          </ul>
        </section>

        <section>
          <h2>目录迁移</h2>
          <ul>
            <li>选中目录后点击“迁移选中项”，无需预先扫描。</li>
            <li>确认目标后，程序会依次复制、校验、切换并建立目录联接。</li>
            <li>迁移前应关闭正在使用该目录的程序和服务。</li>
          </ul>
        </section>

        <section>
          <h2>迁移记录</h2>
          <ul>
            <li>“重新检查”会核对原位置、目标位置和目录联接状态。</li>
            <li>“还原选中项”把数据复制回原磁盘并移除联接。</li>
            <li>异常中断时使用“恢复中断操作”，程序只执行可安全判断的恢复步骤。</li>
          </ul>
        </section>

        <section>
          <h2>链接检测</h2>
          <ul>
            <li>选择目录后，仅检查其直接子项中的符号链接和目录联接。</li>
            <li>检测不会进入链接目标，也不会递归扫描普通子目录。</li>
            <li>空间扫描发现的直接链接也会自动加入记录。</li>
          </ul>
        </section>

        <section>
          <h2>设置</h2>
          <ul>
            <li>自动托管优先选择系统盘同一物理硬盘的最后一个可用 NTFS 分区。</li>
            <li>无法识别物理磁盘时依次回退到 D 盘和 C 盘。</li>
            <li>可设置统一根目录，也可为 AppData、Program Files 等分类单独设置。</li>
          </ul>
        </section>

        <section>
          <h2>权限与安全</h2>
          <ul>
            <li>目录预览、记录、链接检测、设置和帮助无需先扫描。</li>
            <li>MFT 扫描和受保护目录迁移会在需要时请求管理员权限。</li>
            <li>磁盘根目录、系统级根目录、同分区目标和包含链接的目录会被拒绝迁移。</li>
          </ul>
        </section>
      </div>
    </div>
  </section>
</template>

<style scoped>
.view {
  height: 100%;
  overflow: auto;
  padding: var(--space-4) var(--space-5) var(--space-5);
}

.page-header {
  display: flex;
  min-height: 52px;
  align-items: center;
}

h1 {
  font-size: var(--font-size-title);
  font-weight: 650;
}

.help-content {
  display: grid;
  gap: var(--space-4);
}

.quick-start,
.module-grid section {
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-medium);
  background: var(--surface-primary);
}

.quick-start {
  padding: var(--space-4);
}

.module-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-3);
}

.module-grid section {
  min-height: 150px;
  padding: var(--space-4);
}

h2 {
  margin-bottom: var(--space-3);
  font-size: var(--font-size-heading);
  font-weight: 650;
}

ol,
ul {
  display: grid;
  gap: var(--space-2);
  padding-left: 20px;
  color: var(--text-secondary);
  font-size: var(--font-size-small);
  line-height: 1.65;
}

@media (max-width: 1040px) {
  .module-grid {
    grid-template-columns: 1fr;
  }
}
</style>
