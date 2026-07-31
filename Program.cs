using System.ComponentModel;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Security.Principal;
using System.Text.Json;

namespace DirMover;

internal static class Program
{
    [STAThread]
    private static void Main(string[] args)
    {
        if (args.Contains("--self-test"))
        {
            SelfTest.Run();
            return;
        }

        try
        {
            AppStorage.EnsureExists();
            SettingsStore.EnsureExists();
        }
        catch (Exception ex)
        {
            System.Windows.MessageBox.Show(
                $"无法创建配置目录：\n{AppStorage.Root}\n\n{ex.Message}",
                "DirMover", System.Windows.MessageBoxButton.OK,
                System.Windows.MessageBoxImage.Error);
            return;
        }

        var requestedScan = ArgumentValue(args, "--scan");
        if (requestedScan is not null && !IsAdministrator())
        {
            if (TryRestartElevated("--scan", requestedScan))
                return;
            requestedScan = null;
        }

        var application = new System.Windows.Application
        {
            ShutdownMode = System.Windows.ShutdownMode.OnMainWindowClose
        };
        application.Run(new ModernWindow(requestedScan));
    }

    internal static bool IsAdministrator() =>
        new WindowsPrincipal(WindowsIdentity.GetCurrent())
            .IsInRole(WindowsBuiltInRole.Administrator);

    internal static bool TryRestartElevated(params string[] arguments)
    {
        try
        {
            var start = new ProcessStartInfo(Environment.ProcessPath!)
            {
                UseShellExecute = true,
                Verb = "runas"
            };
            foreach (var argument in arguments)
                start.ArgumentList.Add(argument);
            Process.Start(start);
            return true;
        }
        catch (Win32Exception)
        {
            return false;
        }
    }

    private static string? ArgumentValue(string[] args, string name)
    {
        var index = Array.IndexOf(args, name);
        return index >= 0 && index + 1 < args.Length ? args[index + 1] : null;
    }
}

internal sealed class MainForm : Form
{
    private readonly BindingList<FolderRow> folders = [];
    private readonly BindingList<MoveRow> moves = [];
    private readonly BindingList<LinkRow> links = [];
    private readonly DataGridView folderGrid = Grid();
    private readonly DataGridView moveGrid = Grid();
    private readonly DataGridView linkGrid = Grid();
    private readonly ToolStripStatusLabel status = new("就绪");
    private readonly ToolStripProgressBar progress = new() { Style = ProgressBarStyle.Marquee, Visible = false };
    private readonly TextBox rootBox = new() { Dock = DockStyle.Fill, ReadOnly = true };
    private readonly string? startupScan;
    private CancellationTokenSource? scanCancellation;

    public MainForm(string? startupScan = null)
    {
        this.startupScan = startupScan;
        Text = "DirMover — Windows 目录迁移";
        MinimumSize = new Size(900, 620);
        Size = new Size(1100, 720);
        StartPosition = FormStartPosition.CenterScreen;
        Font = new Font("Microsoft YaHei UI", 9F);
        BackColor = Color.FromArgb(246, 248, 251);

        var tabs = new TabControl { Dock = DockStyle.Fill, Padding = new Point(14, 6) };
        tabs.TabPages.Add(BuildSpaceTab());
        tabs.TabPages.Add(BuildMovesTab());
        tabs.TabPages.Add(BuildLinksTab());

        var strip = new StatusStrip();
        strip.Items.Add(status);
        strip.Items.Add(new ToolStripStatusLabel { Spring = true });
        strip.Items.Add(progress);
        Controls.Add(tabs);
        Controls.Add(strip);

        Shown += async (_, _) =>
        {
            RefreshMoves();
            rootBox.Text = startupScan ??
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            if (startupScan is not null)
                await ScanAsync(false);
        };
    }

    private TabPage BuildSpaceTab()
    {
        folderGrid.DataSource = folders;
        folderGrid.Columns.Add(TextColumn(nameof(FolderRow.Name), "文件夹", 220));
        folderGrid.Columns.Add(TextColumn(nameof(FolderRow.SizeText), "逻辑大小", 100));
        folderGrid.Columns.Add(TextColumn(nameof(FolderRow.AllocatedText), "磁盘占用", 100));
        folderGrid.Columns.Add(TextColumn(nameof(FolderRow.FileCount), "文件数", 90));
        folderGrid.Columns.Add(TextColumn(nameof(FolderRow.LinkCount), "内部链接", 80));
        folderGrid.Columns.Add(TextColumn(nameof(FolderRow.Note), "状态", 150, true));
        folderGrid.Columns.Add(TextColumn(nameof(FolderRow.FullPath), "完整路径", 340, true));

        var presets = new ComboBox { DropDownStyle = ComboBoxStyle.DropDownList, Width = 180 };
        presets.Items.AddRange(["本地 AppData", "漫游 AppData", "Program Files", "Program Files (x86)", "自选目录…"]);
        presets.SelectedIndex = 0;

        var scan = Button("扫描大小");
        var cancel = Button("停止");
        var move = Button("迁移选中项", true);

        presets.SelectedIndexChanged += (_, _) =>
        {
            rootBox.Text = presets.SelectedIndex switch
            {
                0 => Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                1 => Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
                2 => Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),
                3 => Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86),
                _ => PickFolder("选择要分析的目录") ?? rootBox.Text
            };
        };
        scan.Click += async (_, _) => await ScanAsync();
        cancel.Click += (_, _) => scanCancellation?.Cancel();
        move.Click += async (_, _) => await MoveSelectedAsync();

        var bar = new FlowLayoutPanel
        {
            Dock = DockStyle.Top, Height = 46, Padding = new Padding(8),
            WrapContents = false, BackColor = Color.White
        };
        bar.Controls.AddRange([presets, scan, cancel, move]);

        var pathPanel = new TableLayoutPanel
        {
            Dock = DockStyle.Top, Height = 38, ColumnCount = 2,
            Padding = new Padding(8, 3, 8, 3), BackColor = Color.White
        };
        pathPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 72));
        pathPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
        pathPanel.Controls.Add(new Label { Text = "扫描位置", AutoSize = true, Anchor = AnchorStyles.Left }, 0, 0);
        pathPanel.Controls.Add(rootBox, 1, 0);

        return Tab("空间分析", folderGrid, pathPanel, bar);
    }

    private TabPage BuildMovesTab()
    {
        moveGrid.DataSource = moves;
        moveGrid.Columns.Add(TextColumn(nameof(MoveRow.Status), "状态", 150));
        moveGrid.Columns.Add(TextColumn(nameof(MoveRow.Source), "原位置", 330, true));
        moveGrid.Columns.Add(TextColumn(nameof(MoveRow.Destination), "新位置", 330, true));
        moveGrid.Columns.Add(TextColumn(nameof(MoveRow.Created), "迁移时间", 150));

        var refresh = Button("重新检查");
        var restore = Button("还原选中项", true);
        var openStore = Button("打开记录位置");
        refresh.Click += (_, _) => RefreshMoves();
        restore.Click += async (_, _) => await RestoreSelectedAsync();
        openStore.Click += (_, _) => Process.Start(new ProcessStartInfo("explorer.exe", $"/select,\"{MoveStore.FilePath}\"")
        {
            UseShellExecute = true
        });

        var bar = new FlowLayoutPanel
        {
            Dock = DockStyle.Top, Height = 46, Padding = new Padding(8),
            WrapContents = false, BackColor = Color.White
        };
        bar.Controls.AddRange([refresh, restore, openStore]);
        return Tab("迁移记录", moveGrid, bar);
    }

    private TabPage BuildLinksTab()
    {
        linkGrid.DataSource = links;
        linkGrid.Columns.Add(TextColumn(nameof(LinkRow.Kind), "类型", 100));
        linkGrid.Columns.Add(TextColumn(nameof(LinkRow.Path), "链接位置", 390, true));
        linkGrid.Columns.Add(TextColumn(nameof(LinkRow.Target), "指向位置", 390, true));
        linkGrid.Columns.Add(TextColumn(nameof(LinkRow.State), "状态", 100));

        var detect = Button("选择目录并检测", true);
        detect.Click += async (_, _) =>
        {
            var root = PickFolder("选择要递归检测软链接/目录联接的位置");
            if (root is not null)
                await DetectLinksAsync(root);
        };
        var hint = new Label
        {
            Text = "检测目录联接和符号链接；扫描时不会进入链接目标，避免循环。",
            AutoSize = true, Margin = new Padding(16, 9, 0, 0), ForeColor = Color.DimGray
        };
        var bar = new FlowLayoutPanel
        {
            Dock = DockStyle.Top, Height = 46, Padding = new Padding(8),
            WrapContents = false, BackColor = Color.White
        };
        bar.Controls.AddRange([detect, hint]);
        return Tab("链接检测", linkGrid, bar);
    }

    private async Task<bool> ScanAsync(bool requestElevation = true)
    {
        if (!Directory.Exists(rootBox.Text))
        {
            MessageBox.Show("扫描目录不存在。", "DirMover", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            return false;
        }
        if (requestElevation && !Program.IsAdministrator())
        {
            if (Program.TryRestartElevated("--scan", rootBox.Text))
                Close();
            return false;
        }

        scanCancellation?.Cancel();
        scanCancellation = new CancellationTokenSource();
        folders.Clear();
        Busy($"正在扫描 {rootBox.Text}…", true);
        var found = new List<FolderRow>();
        try
        {
            var snapshot = await Scanner.ScanChildrenAsync(
                rootBox.Text, new Progress<ScanProgress>(), scanCancellation.Token);
            found = snapshot.Folders;
            folders.RaiseListChangedEvents = false;
            folders.Clear();
            foreach (var row in found.OrderByDescending(x => x.Bytes))
                folders.Add(row);
            folders.RaiseListChangedEvents = true;
            folders.ResetBindings();
            var engine = found.All(x => x.Note.StartsWith("MFT")) ? "NTFS MFT" : "兼容模式";
            Busy($"扫描完成（{engine}）：{found.Count} 个文件夹，共 {SizeText(found.Sum(x => x.Bytes))}", false);
            return true;
        }
        catch (OperationCanceledException)
        {
            Busy("扫描已停止", false);
            return false;
        }
        catch (Exception ex)
        {
            Busy("扫描失败", false);
            MessageBox.Show(ex.Message, "扫描失败", MessageBoxButtons.OK, MessageBoxIcon.Error);
            return false;
        }
    }

    private async Task MoveSelectedAsync()
    {
        if (folderGrid.CurrentRow?.DataBoundItem is not FolderRow row)
        {
            MessageBox.Show("请先选择一个文件夹。", "DirMover");
            return;
        }
        if (!Program.IsAdministrator())
        {
            if (Program.TryRestartElevated("--scan", rootBox.Text))
                Close();
            return;
        }
        if (row.IsLink)
        {
            MessageBox.Show("选中项已经是链接，不能再次迁移。", "DirMover");
            return;
        }
        if (row.LinkCount > 0)
        {
            MessageBox.Show("此文件夹内部含有目录联接或符号链接。为避免遗漏数据，DirMover 不会迁移它；请先在“链接检测”中处理这些链接。",
                "安全检查未通过", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            return;
        }

        var basePath = PickFolder("选择目标磁盘或目录（将在其下创建 DirMover 文件夹）");
        if (basePath is null) return;
        var parentName = Path.GetFileName(Path.GetDirectoryName(row.FullPath)) ?? "Folders";
        var destination = Path.Combine(basePath, "DirMover", parentName, row.Name);

        var answer = MessageBox.Show(
            $"将要迁移：\n{row.FullPath}\n\n到：\n{destination}\n\n复制和校验完成后，原位置会变成目录联接。请先关闭正在使用此目录的程序。",
            "确认迁移", MessageBoxButtons.OKCancel, MessageBoxIcon.Warning);
        if (answer != DialogResult.OK) return;

        Busy("正在安全复制、校验并创建目录联接，请勿关闭程序…", true);
        try
        {
            var result = await MoveEngine.MoveAsync(row.FullPath, destination);
            RefreshMoves();
            Busy("迁移完成", false);
            MessageBox.Show(result, "迁移完成", MessageBoxButtons.OK, MessageBoxIcon.Information);
            await ScanAsync();
        }
        catch (Exception ex)
        {
            RefreshMoves();
            Busy("迁移未完成", false);
            MessageBox.Show(ex.Message, "迁移失败", MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }

    private async Task RestoreSelectedAsync()
    {
        if (moveGrid.CurrentRow?.DataBoundItem is not MoveRow row) return;
        if (!Program.IsAdministrator())
        {
            MessageBox.Show("还原需要管理员权限。确认 UAC 后，请重新选择迁移记录。",
                "DirMover", MessageBoxButtons.OK, MessageBoxIcon.Information);
            if (Program.TryRestartElevated())
                Close();
            return;
        }
        var record = MoveStore.Load().FirstOrDefault(x => x.Id == row.Id);
        if (record is null) return;
        if (!MoveEngine.IsActive(record))
        {
            MessageBox.Show("该记录目前不是有效链接，不能自动还原。数据不会被改动。", "DirMover",
                MessageBoxButtons.OK, MessageBoxIcon.Warning);
            return;
        }
        if (MessageBox.Show($"把数据还原到：\n{record.Source}\n\n还原需要原磁盘有足够空间。",
                "确认还原", MessageBoxButtons.OKCancel, MessageBoxIcon.Warning) != DialogResult.OK) return;

        Busy("正在复制回原磁盘并校验…", true);
        try
        {
            var message = await MoveEngine.RestoreAsync(record);
            RefreshMoves();
            Busy("还原完成", false);
            MessageBox.Show(message, "还原完成", MessageBoxButtons.OK, MessageBoxIcon.Information);
        }
        catch (Exception ex)
        {
            RefreshMoves();
            Busy("还原未完成", false);
            MessageBox.Show(ex.Message, "还原失败", MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }

    private async Task DetectLinksAsync(string root)
    {
        links.Clear();
        Busy($"正在检测 {root}…", true);
        try
        {
            foreach (var link in await Scanner.FindLinksAsync(root, CancellationToken.None))
                links.Add(link);
            Busy($"检测完成：找到 {links.Count} 个链接", false);
        }
        catch (Exception ex)
        {
            Busy("检测失败", false);
            MessageBox.Show(ex.Message, "检测失败", MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }

    private void RefreshMoves()
    {
        moves.Clear();
        foreach (var record in MoveStore.Load().OrderByDescending(x => x.CreatedAtUtc))
            moves.Add(new MoveRow(record, MoveEngine.StatusOf(record)));
        status.Text = moves.Count == 0 ? "没有迁移记录" : $"已检查 {moves.Count} 条迁移记录";
    }

    private void Busy(string message, bool isBusy)
    {
        status.Text = message;
        progress.Visible = isBusy;
        UseWaitCursor = isBusy;
    }

    private string? PickFolder(string description)
    {
        using var dialog = new FolderBrowserDialog
        {
            Description = description,
            UseDescriptionForTitle = true,
            ShowNewFolderButton = true
        };
        return dialog.ShowDialog(this) == DialogResult.OK ? dialog.SelectedPath : null;
    }

    private static TabPage Tab(string title, Control fill, params Control[] topControls)
    {
        var page = new TabPage(title) { BackColor = Color.White };
        page.Controls.Add(fill);
        foreach (var control in topControls)
            page.Controls.Add(control);
        return page;
    }

    private static Button Button(string text, bool primary = false) => new()
    {
        Text = text,
        AutoSize = true,
        Height = 30,
        FlatStyle = FlatStyle.Flat,
        BackColor = primary ? Color.FromArgb(36, 99, 235) : Color.White,
        ForeColor = primary ? Color.White : Color.FromArgb(30, 41, 59),
        Margin = new Padding(4, 0, 4, 0),
        Padding = new Padding(8, 0, 8, 0),
        UseVisualStyleBackColor = false
    };

    private static DataGridView Grid() => new()
    {
        Dock = DockStyle.Fill,
        AutoGenerateColumns = false,
        AllowUserToAddRows = false,
        AllowUserToDeleteRows = false,
        AllowUserToResizeRows = false,
        ReadOnly = true,
        MultiSelect = false,
        SelectionMode = DataGridViewSelectionMode.FullRowSelect,
        BackgroundColor = Color.White,
        BorderStyle = BorderStyle.None,
        RowHeadersVisible = false,
        AutoSizeRowsMode = DataGridViewAutoSizeRowsMode.None,
        ColumnHeadersHeight = 36,
        RowTemplate = { Height = 32 }
    };

    private static DataGridViewTextBoxColumn TextColumn(string property, string title, int width, bool fill = false) => new()
    {
        DataPropertyName = property,
        HeaderText = title,
        Width = width,
        AutoSizeMode = fill ? DataGridViewAutoSizeColumnMode.Fill : DataGridViewAutoSizeColumnMode.None
    };

    internal static string SizeText(long bytes)
    {
        string[] units = ["B", "KB", "MB", "GB", "TB"];
        double value = bytes;
        var unit = 0;
        while (value >= 1024 && unit < units.Length - 1)
        {
            value /= 1024;
            unit++;
        }
        return $"{value:0.##} {units[unit]}";
    }
}

internal static class Scanner
{
    public static Task<List<FolderRow>> ListChildFoldersAsync(
        string root, CancellationToken cancellationToken) =>
        Task.Run(() =>
        {
            var rows = new List<FolderRow>();
            foreach (var path in Directory.EnumerateDirectories(root))
            {
                cancellationToken.ThrowIfCancellationRequested();
                try
                {
                    var attributes = File.GetAttributes(path);
                    var isLink = attributes.HasFlag(FileAttributes.ReparsePoint);
                    var target = isLink ? LinkTarget(path) : null;
                    rows.Add(new FolderRow(
                        Path.GetFileName(path), path, 0, 0, 0, isLink ? 1 : 0, 0, isLink,
                        isLink
                            ? target is null ? "目录链接（目标不可读）" : $"目录链接 → {target}"
                            : "未扫描",
                        false));
                }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
                {
                    rows.Add(new FolderRow(
                        Path.GetFileName(path), path, 0, 0, 0, 0, 1, false,
                        "未扫描（属性不可读）", false));
                }
            }
            return rows;
        }, cancellationToken);

    public static async Task<ScanSnapshot> ScanChildrenAsync(
        string root, IProgress<ScanProgress> scanProgress,
        CancellationToken cancellationToken)
    {
        try
        {
            var result = await Task.Run(
                () => MftScanner.ScanChildren(root, scanProgress, cancellationToken),
                cancellationToken);
            return result;
        }
        catch (OperationCanceledException) { throw; }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or NotSupportedException)
        {
            return await ScanChildrenByEnumerationAsync(root, scanProgress, cancellationToken);
        }
    }

    private static async Task<ScanSnapshot> ScanChildrenByEnumerationAsync(
        string root, IProgress<ScanProgress> scanProgress,
        CancellationToken cancellationToken)
    {
        var directories = Directory.EnumerateDirectories(root).ToList();
        var rows = new System.Collections.Concurrent.ConcurrentBag<FolderRow>();
        var links = new System.Collections.Concurrent.ConcurrentBag<LinkRow>();
        var completed = 0;
        scanProgress.Report(new ScanProgress(5, "正在使用兼容扫描…"));
        await Parallel.ForEachAsync(directories,
            new ParallelOptions
            {
                CancellationToken = cancellationToken,
                MaxDegreeOfParallelism = Math.Clamp(Environment.ProcessorCount, 2, 8)
            },
            (path, token) =>
            {
                token.ThrowIfCancellationRequested();
                FolderRow row;
                try
                {
                    var attributes = File.GetAttributes(path);
                    if (attributes.HasFlag(FileAttributes.ReparsePoint))
                    {
                        var link = CreateLinkRow(path, true, root);
                        links.Add(link);
                        row = new FolderRow(Path.GetFileName(path), path, 0, 0, 0, 1, 0, true,
                            link.Target == "无法解析" ? "兼容扫描：链接（目标不可读）" :
                            $"兼容扫描：链接 → {link.Target}");
                    }
                    else
                    {
                        var stats = Measure(path, token);
                        var note = stats.Errors == 0 ? "兼容扫描" :
                            $"兼容扫描：{stats.Errors} 项无法读取";
                        row = new FolderRow(Path.GetFileName(path), path, stats.Bytes, stats.Bytes,
                            stats.Files, 0, stats.Errors, false, note);
                    }
                }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
                {
                    row = new FolderRow(Path.GetFileName(path), path, 0, 0, 0, 0, 1, false,
                        "兼容扫描：无法读取");
                }
                rows.Add(row);
                var done = Interlocked.Increment(ref completed);
                scanProgress.Report(new ScanProgress(
                    directories.Count == 0 ? 100 : 5 + done * 94 / directories.Count,
                    $"正在扫描文件夹 {done:N0}/{directories.Count:N0}"));
                return ValueTask.CompletedTask;
            });
        scanProgress.Report(new ScanProgress(100, "扫描完成"));
        return new ScanSnapshot(rows.ToList(), links.ToList(), "兼容模式");
    }

    public static FolderStats Measure(
        string root, CancellationToken cancellationToken = default,
        Action<string, bool>? linkFound = null)
    {
        long bytes = 0, files = 0, links = 0, errors = 0;
        var stack = new Stack<string>();
        stack.Push(root);
        while (stack.Count > 0)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var current = stack.Pop();
            IEnumerable<string> entries;
            try
            {
                entries = Directory.EnumerateFileSystemEntries(current);
            }
            catch
            {
                errors++;
                continue;
            }
            try
            {
                foreach (var entry in entries)
                {
                    cancellationToken.ThrowIfCancellationRequested();
                    try
                    {
                        var attributes = File.GetAttributes(entry);
                        if (attributes.HasFlag(FileAttributes.ReparsePoint))
                        {
                            links++;
                            linkFound?.Invoke(entry, attributes.HasFlag(FileAttributes.Directory));
                        }
                        else if (attributes.HasFlag(FileAttributes.Directory))
                        {
                            stack.Push(entry);
                        }
                        else
                        {
                            bytes += new FileInfo(entry).Length;
                            files++;
                        }
                    }
                    catch { errors++; }
                }
            }
            catch { errors++; }
        }
        return new FolderStats(bytes, files, links, errors);
    }

    public static Task<List<LinkRow>> FindLinksAsync(string root, CancellationToken token) =>
        Task.Run(() =>
        {
            var found = new List<LinkRow>();
            IEnumerable<string> entries;
            try { entries = Directory.EnumerateFileSystemEntries(root); }
            catch { return found; }
            foreach (var entry in entries)
            {
                token.ThrowIfCancellationRequested();
                try
                {
                    var attributes = File.GetAttributes(entry);
                    if (attributes.HasFlag(FileAttributes.ReparsePoint))
                        found.Add(CreateLinkRow(
                            entry, attributes.HasFlag(FileAttributes.Directory), root));
                }
                catch { }
            }
            return found;
        }, token);

    internal static LinkRow CreateLinkRow(string path, bool isDirectory, string scanRoot)
    {
        var target = LinkTarget(path);
        return new LinkRow(
            isDirectory ? "目录链接" : "文件链接",
            path,
            target ?? "无法解析",
            target is not null && (Directory.Exists(target) || File.Exists(target)) ? "有效" : "失效",
            Path.GetFullPath(scanRoot),
            DateTime.UtcNow);
    }

    public static string? LinkTarget(string path)
    {
        try
        {
            FileSystemInfo info = new DirectoryInfo(path);
            return info.ResolveLinkTarget(false)?.FullName ?? info.LinkTarget;
        }
        catch
        {
            try
            {
                FileSystemInfo info = new FileInfo(path);
                return info.ResolveLinkTarget(false)?.FullName ?? info.LinkTarget;
            }
            catch { return null; }
        }
    }
}

internal static class MoveEngine
{
    public static async Task<string> MoveAsync(string source, string destination)
    {
        source = Path.GetFullPath(source);
        destination = Path.GetFullPath(destination);
        Validate(source, destination);

        var sourceStats = await Task.Run(() => Scanner.Measure(source));
        if (sourceStats.Errors > 0)
            throw new InvalidOperationException($"源目录有 {sourceStats.Errors} 项无法读取，已停止迁移。");
        if (sourceStats.Links > 0)
            throw new InvalidOperationException("源目录内部含链接，已停止迁移，避免链接内容遗漏。");

        var targetDrive = new DriveInfo(Path.GetPathRoot(destination)!);
        if (targetDrive.AvailableFreeSpace < sourceStats.Bytes + 64L * 1024 * 1024)
            throw new InvalidOperationException("目标磁盘空间不足（需额外预留 64 MB）。");

        var record = new MoveRecord
        {
            Id = Guid.NewGuid(),
            Source = source,
            Destination = destination,
            CreatedAtUtc = DateTime.UtcNow,
            Stage = "Preparing"
        };
        MoveStore.Add(record);

        var backup = $"{source}.dirmover-backup-{record.Id:N}";
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(destination)!);
            await CopyDirectoryAsync(source, destination);
            Verify(sourceStats, Scanner.Measure(destination), "首次复制");

            Directory.Move(source, backup);
            record.BackupPath = backup;
            MoveStore.Update(record);

            await CopyDirectoryAsync(backup, destination);
            Verify(Scanner.Measure(backup), Scanner.Measure(destination), "最终同步");

            CreateJunction(source, destination);
            if (!PointsTo(source, destination))
                throw new IOException("目录联接创建后校验失败。");

            record.Stage = "Active";
            MoveStore.Update(record);

            try
            {
                Directory.Delete(backup, true);
                record.BackupPath = null;
                MoveStore.Update(record);
                return $"已迁移 {MainForm.SizeText(sourceStats.Bytes)}。\n原位置现已安全指向新位置。";
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                return $"迁移和链接已生效，但旧数据备份暂时无法删除：\n{backup}\n\n可能仍有程序占用它。请关闭相关程序后手动删除此备份。";
            }
        }
        catch (Exception ex)
        {
            if (Directory.Exists(backup))
            {
                try
                {
                    if (Directory.Exists(source) &&
                        File.GetAttributes(source).HasFlag(FileAttributes.ReparsePoint))
                        Directory.Delete(source);
                    if (!Directory.Exists(source))
                        Directory.Move(backup, source);
                }
                catch (Exception rollback)
                {
                    record.Stage = "RollbackFailed";
                    MoveStore.Update(record);
                    throw new IOException($"迁移失败，且自动回滚失败。请勿删除以下目录：\n{backup}\n{destination}", rollback);
                }
            }
            if (Directory.Exists(destination) && Directory.Exists(source) &&
                !File.GetAttributes(source).HasFlag(FileAttributes.ReparsePoint))
            {
                try { Directory.Delete(destination, true); } catch { }
            }
            MoveStore.Remove(record.Id);
            throw new IOException($"源目录保持不变，迁移已停止。\n\n原因：{ex.Message}", ex);
        }
    }

    public static async Task<string> RestoreAsync(MoveRecord record)
    {
        if (!IsActive(record))
            throw new InvalidOperationException("原位置不是此记录对应的有效链接。");
        if (!string.IsNullOrEmpty(record.BackupPath) && Directory.Exists(record.BackupPath))
            throw new InvalidOperationException($"仍有迁移备份未清理，请先关闭相关程序并删除：\n{record.BackupPath}");

        var staging = $"{record.Source}.dirmover-restore-{record.Id:N}";
        if (Directory.Exists(staging))
            throw new IOException($"还原暂存目录已存在，请先检查：\n{staging}");

        record.Stage = "Restoring";
        MoveStore.Update(record);
        try
        {
            var targetStats = await Task.Run(() => Scanner.Measure(record.Destination));
            var sourceDrive = new DriveInfo(Path.GetPathRoot(record.Source)!);
            if (sourceDrive.AvailableFreeSpace < targetStats.Bytes + 64L * 1024 * 1024)
                throw new IOException("原磁盘空间不足（需额外预留 64 MB）。");

            await CopyDirectoryAsync(record.Destination, staging);
            Verify(targetStats, Scanner.Measure(staging), "还原复制");

            Directory.Delete(record.Source);
            try
            {
                Directory.Move(staging, record.Source);
            }
            catch
            {
                CreateJunction(record.Source, record.Destination);
                throw;
            }

            try
            {
                Directory.Delete(record.Destination, true);
                MoveStore.Remove(record.Id);
                return "数据已还原，迁移记录已移除。";
            }
            catch
            {
                record.Stage = "Restored";
                MoveStore.Update(record);
                return $"数据已还原，但新位置的副本未能删除：\n{record.Destination}\n\n确认原位置工作正常后可手动删除它。";
            }
        }
        catch
        {
            if (Directory.Exists(staging) && Directory.Exists(record.Source))
                try { Directory.Delete(staging, true); } catch { }
            if (IsActive(record))
            {
                record.Stage = "Active";
                MoveStore.Update(record);
            }
            throw;
        }
    }

    public static bool IsActive(MoveRecord record) =>
        Directory.Exists(record.Destination) && PointsTo(record.Source, record.Destination);

    public static string StatusOf(MoveRecord record)
    {
        if (IsActive(record))
            return !string.IsNullOrEmpty(record.BackupPath) && Directory.Exists(record.BackupPath)
                ? "有效（备份待清理）" : "有效";
        if (record.Stage == "Restored" && Directory.Exists(record.Source))
            return "已还原（副本待清理）";
        if (!Directory.Exists(record.Source) && !Directory.Exists(record.Destination))
            return "两处均不存在";
        if (!Directory.Exists(record.Source))
            return "原链接丢失";
        if (!Directory.Exists(record.Destination))
            return "目标丢失";
        if (File.GetAttributes(record.Source).HasFlag(FileAttributes.ReparsePoint))
            return "链接指向已改变";
        return record.Stage == "Preparing" ? "迁移中断/未完成" : "原位置已是普通目录";
    }

    internal static void Validate(string source, string destination)
    {
        if (!OperatingSystem.IsWindows())
            throw new PlatformNotSupportedException("DirMover 仅支持 Windows。");
        if (!Directory.Exists(source))
            throw new DirectoryNotFoundException("源目录不存在。");
        if (Directory.Exists(destination) || File.Exists(destination))
            throw new IOException("目标目录已经存在。请选择其他位置。");
        if (File.GetAttributes(source).HasFlag(FileAttributes.ReparsePoint))
            throw new IOException("源目录已经是链接。");
        if (SameOrInside(source, destination) || SameOrInside(destination, source))
            throw new IOException("源目录和目标目录不能互相包含。");

        var sourceRoot = Path.GetPathRoot(source)!;
        if (PathEquals(source, sourceRoot))
            throw new IOException("不能迁移整个磁盘根目录。");

        string[] protectedRoots =
        [
            Environment.GetFolderPath(Environment.SpecialFolder.Windows),
            Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),
            Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86),
            Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData)
        ];
        if (protectedRoots.Where(x => !string.IsNullOrWhiteSpace(x)).Any(x => PathEquals(source, x)))
            throw new IOException("不能迁移整个系统级目录；请选择其中的子文件夹。");
        if (SameOrInside(AppContext.BaseDirectory, source) ||
            SameOrInside(AppStorage.Root, source))
            throw new IOException("不能迁移 DirMover 程序或记录所在的目录。");

        var sourceDrive = new DriveInfo(sourceRoot);
        var targetDrive = new DriveInfo(Path.GetPathRoot(destination)!);
        if (!sourceDrive.IsReady || !targetDrive.IsReady ||
            !sourceDrive.DriveFormat.Equals("NTFS", StringComparison.OrdinalIgnoreCase) ||
            !targetDrive.DriveFormat.Equals("NTFS", StringComparison.OrdinalIgnoreCase))
            throw new IOException("源位置和目标位置都必须位于可用的本地 NTFS 磁盘上。");
        if (PathEquals(sourceDrive.RootDirectory.FullName, targetDrive.RootDirectory.FullName))
            throw new IOException("目标位置与源目录在同一磁盘，无法释放该磁盘空间。");
        if (MoveStore.Load().Any(x => PathEquals(x.Source, source)))
            throw new IOException("此源目录已有迁移记录。");
    }

    internal static bool SameOrInside(string path, string parent)
    {
        var relative = Path.GetRelativePath(Path.GetFullPath(parent), Path.GetFullPath(path));
        return relative == "." || (!relative.StartsWith(".." + Path.DirectorySeparatorChar) &&
                                    relative != ".." && !Path.IsPathRooted(relative));
    }

    private static async Task CopyDirectoryAsync(string source, string destination)
    {
        var arguments = new[]
        {
            source, destination, "/E", "/COPYALL", "/DCOPY:DAT", "/XJ",
            "/R:1", "/W:1", "/NP", "/NFL", "/NDL", "/NJH", "/NJS"
        };
        var result = await RunAsync("robocopy.exe", arguments);
        if (result.ExitCode > 7)
            throw new IOException($"Robocopy 返回错误码 {result.ExitCode}。\n{result.Error}".Trim());
    }

    private static void Verify(FolderStats expected, FolderStats actual, string stage)
    {
        if (expected.Errors > 0 || actual.Errors > 0 ||
            expected.Files != actual.Files || expected.Bytes != actual.Bytes ||
            expected.Links != actual.Links)
            throw new IOException($"{stage}校验失败：文件数量或大小不一致。");
    }

    private static void CreateJunction(string source, string destination)
    {
        var result = RunAsync("cmd.exe", ["/d", "/c", "mklink", "/J", source, destination])
            .GetAwaiter().GetResult();
        if (result.ExitCode != 0)
            throw new IOException($"无法创建目录联接：{result.Error}");
    }

    private static bool PointsTo(string source, string destination)
    {
        var target = Scanner.LinkTarget(source);
        return target is not null && PathEquals(target, destination);
    }

    private static bool PathEquals(string left, string right) =>
        string.Equals(Path.TrimEndingDirectorySeparator(Path.GetFullPath(left)),
            Path.TrimEndingDirectorySeparator(Path.GetFullPath(right)),
            StringComparison.OrdinalIgnoreCase);

    private static async Task<ProcessResult> RunAsync(string fileName, IEnumerable<string> arguments)
    {
        var start = new ProcessStartInfo(fileName)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true
        };
        foreach (var argument in arguments)
            start.ArgumentList.Add(argument);
        using var process = Process.Start(start) ?? throw new IOException($"无法启动 {fileName}。");
        var output = process.StandardOutput.ReadToEndAsync();
        var error = process.StandardError.ReadToEndAsync();
        await process.WaitForExitAsync();
        return new ProcessResult(process.ExitCode, await output, await error);
    }
}

internal static class MoveStore
{
    public static readonly string Root = AppStorage.Root;
    public static readonly string FilePath = Path.Combine(Root, "moves.json");
    private static readonly JsonSerializerOptions JsonOptions = new() { WriteIndented = true };
    private static readonly object Gate = new();

    public static List<MoveRecord> Load()
    {
        lock (Gate)
        {
            try
            {
                return File.Exists(FilePath)
                    ? JsonSerializer.Deserialize<List<MoveRecord>>(File.ReadAllText(FilePath)) ?? []
                    : [];
            }
            catch (JsonException ex)
            {
                throw new IOException($"迁移记录损坏，请检查：\n{FilePath}", ex);
            }
        }
    }

    public static void Add(MoveRecord record)
    {
        var records = Load();
        records.Add(record);
        Save(records);
    }

    public static void Update(MoveRecord record)
    {
        var records = Load();
        var index = records.FindIndex(x => x.Id == record.Id);
        if (index < 0) records.Add(record); else records[index] = record;
        Save(records);
    }

    public static void Remove(Guid id)
    {
        var records = Load();
        records.RemoveAll(x => x.Id == id);
        Save(records);
    }

    private static void Save(List<MoveRecord> records)
    {
        lock (Gate)
        {
            AppStorage.EnsureExists();
            var temporary = FilePath + ".tmp";
            File.WriteAllText(temporary, JsonSerializer.Serialize(records, JsonOptions));
            File.Move(temporary, FilePath, true);
        }
    }
}

internal static class LinkStore
{
    public static readonly string FilePath = Path.Combine(AppStorage.Root, "links.json");
    private static readonly JsonSerializerOptions JsonOptions = new() { WriteIndented = true };
    private static readonly object Gate = new();

    public static List<LinkRow> Load()
    {
        lock (Gate)
        {
            try
            {
                return File.Exists(FilePath)
                    ? JsonSerializer.Deserialize<List<LinkRow>>(File.ReadAllText(FilePath)) ?? []
                    : [];
            }
            catch (JsonException ex)
            {
                throw new IOException($"链接记录损坏，请检查：\n{FilePath}", ex);
            }
        }
    }

    public static void ReplaceForRoot(string root, IEnumerable<LinkRow> found)
    {
        root = Path.GetFullPath(root);
        var records = Load();
        records.RemoveAll(x => string.Equals(
            Path.TrimEndingDirectorySeparator(x.ScanRoot),
            Path.TrimEndingDirectorySeparator(root),
            StringComparison.OrdinalIgnoreCase));
        records.AddRange(found);
        lock (Gate)
        {
            AppStorage.EnsureExists();
            var temporary = FilePath + ".tmp";
            File.WriteAllText(temporary, JsonSerializer.Serialize(records, JsonOptions));
            File.Move(temporary, FilePath, true);
        }
    }
}

internal static class SettingsStore
{
    public static readonly string FilePath = Path.Combine(AppStorage.Root, "settings.json");
    private static readonly JsonSerializerOptions JsonOptions = new() { WriteIndented = true };
    private static readonly object Gate = new();

    public static void EnsureExists()
    {
        lock (Gate)
        {
            if (!File.Exists(FilePath))
                SaveCore(new AppSettings());
        }
    }

    public static AppSettings Load()
    {
        lock (Gate)
        {
            try
            {
                var settings = File.Exists(FilePath)
                    ? JsonSerializer.Deserialize<AppSettings>(File.ReadAllText(FilePath)) ??
                      new AppSettings()
                    : new AppSettings();
                settings.CategoryRoots ??= [];
                return settings;
            }
            catch (JsonException ex)
            {
                throw new IOException($"设置文件损坏，请检查：\n{FilePath}", ex);
            }
        }
    }

    public static void Save(AppSettings settings)
    {
        lock (Gate)
            SaveCore(settings);
    }

    private static void SaveCore(AppSettings settings)
    {
        AppStorage.EnsureExists();
        var temporary = FilePath + ".tmp";
        File.WriteAllText(temporary, JsonSerializer.Serialize(settings, JsonOptions));
        File.Move(temporary, FilePath, true);
    }
}

internal static class StoragePlanner
{
    public const string ManagedFolderName = "DirMoverData";
    public static readonly IReadOnlyList<ManagedCategoryDefinition> Categories =
    [
        new("Local", "AppData Local",
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            Path.Combine("AppData", "Local")),
        new("LocalLow", "AppData LocalLow",
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
                "AppData", "LocalLow"),
            Path.Combine("AppData", "LocalLow")),
        new("Roaming", "AppData Roaming",
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            Path.Combine("AppData", "Roaming")),
        new("ProgramFilesX86", "Program Files (x86)",
            Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86),
            "Program Files (x86)"),
        new("ProgramFiles", "Program Files",
            Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),
            "Program Files"),
        new("ProgramData", "ProgramData",
            Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
            "ProgramData"),
        new("Other", "其他目录", null, "Other")
    ];

    public static List<DriveInfo> FixedNtfsDrives() =>
        DriveInfo.GetDrives()
            .Where(drive =>
            {
                try
                {
                    return drive.DriveType == DriveType.Fixed && drive.IsReady &&
                           drive.DriveFormat.Equals("NTFS", StringComparison.OrdinalIgnoreCase);
                }
                catch
                {
                    return false;
                }
            })
            .OrderBy(drive => drive.Name, StringComparer.OrdinalIgnoreCase)
            .ToList();

    public static bool HasOnlySystemDrive()
    {
        var drives = FixedNtfsDrives();
        var systemRoot = Path.GetPathRoot(Environment.SystemDirectory)!;
        return drives.Count == 1 && PathEquals(drives[0].RootDirectory.FullName, systemRoot);
    }

    public static string? AutomaticManagedRoot()
    {
        var drives = FixedNtfsDrives();
        if (drives.Count == 0 || HasOnlySystemDrive())
            return null;
        return Path.Combine(drives[^1].RootDirectory.FullName, ManagedFolderName);
    }

    public static string? EffectiveManagedRoot()
    {
        var configured = SettingsStore.Load().ManagedRoot;
        return string.IsNullOrWhiteSpace(configured)
            ? AutomaticManagedRoot()
            : Path.GetFullPath(configured);
    }

    public static ManagedCategoryDefinition CategoryForSource(string source)
    {
        source = Path.GetFullPath(source);
        return Categories.FirstOrDefault(category =>
                   !string.IsNullOrWhiteSpace(category.SourceRoot) &&
                   MoveEngine.SameOrInside(source, category.SourceRoot)) ??
               Categories[^1];
    }

    public static string? CategoryRoot(string categoryKey)
    {
        var settings = SettingsStore.Load();
        if (settings.CategoryRoots.TryGetValue(categoryKey, out var overridden) &&
            !string.IsNullOrWhiteSpace(overridden))
            return Path.GetFullPath(overridden);

        var baseRoot = string.IsNullOrWhiteSpace(settings.ManagedRoot)
            ? AutomaticManagedRoot()
            : Path.GetFullPath(settings.ManagedRoot);
        var category = Categories.First(x =>
            x.Key.Equals(categoryKey, StringComparison.OrdinalIgnoreCase));
        return baseRoot is null ? null : Path.Combine(baseRoot, category.DefaultRelativeRoot);
    }

    public static string? CategoryRootForSource(string source) =>
        CategoryRoot(CategoryForSource(source).Key);

    public static string? ManagedDestinationFor(string source)
    {
        var category = CategoryForSource(source);
        var categoryRoot = CategoryRoot(category.Key);
        return categoryRoot is null
            ? null
            : DestinationUnderCategory(categoryRoot, source, category);
    }

    public static bool CanUseForSource(string? managedRoot, string source, out string reason)
    {
        if (!IsValidManagedRoot(managedRoot, out reason))
            return false;

        try
        {
            var sourceDrive = new DriveInfo(Path.GetPathRoot(Path.GetFullPath(source))!);
            var targetDrive = new DriveInfo(Path.GetPathRoot(Path.GetFullPath(managedRoot!))!);
            if (PathEquals(sourceDrive.RootDirectory.FullName, targetDrive.RootDirectory.FullName))
            {
                reason = "托管位置与源目录在同一分区，无法释放源磁盘空间。";
                return false;
            }
            reason = "";
            return true;
        }
        catch (Exception ex) when (ex is IOException or ArgumentException or UnauthorizedAccessException)
        {
            reason = $"托管位置不可用：{ex.Message}";
            return false;
        }
    }

    public static bool IsValidManagedRoot(string? managedRoot, out string reason)
    {
        if (string.IsNullOrWhiteSpace(managedRoot))
        {
            reason = HasOnlySystemDrive()
                ? "只检测到系统盘，默认托管已停用。请接入其他 NTFS 磁盘并自定义位置。"
                : "没有可用的默认托管磁盘。";
            return false;
        }
        try
        {
            var targetDrive = new DriveInfo(Path.GetPathRoot(Path.GetFullPath(managedRoot))!);
            if (!targetDrive.IsReady ||
                !targetDrive.DriveFormat.Equals("NTFS", StringComparison.OrdinalIgnoreCase))
            {
                reason = "托管位置必须位于可用的 NTFS 磁盘。";
                return false;
            }
            reason = "";
            return true;
        }
        catch (Exception ex) when (ex is IOException or ArgumentException or UnauthorizedAccessException)
        {
            reason = $"托管位置不可用：{ex.Message}";
            return false;
        }
    }

    public static string DestinationUnderBase(string managedRoot, string source)
    {
        source = Path.GetFullPath(source);
        managedRoot = Path.GetFullPath(managedRoot);
        var category = CategoryForSource(source);
        return DestinationUnderCategory(
            Path.Combine(managedRoot, category.DefaultRelativeRoot), source, category);
    }

    public static string DestinationUnderCategory(
        string categoryRoot, string source, ManagedCategoryDefinition? category = null)
    {
        source = Path.GetFullPath(source);
        categoryRoot = Path.GetFullPath(categoryRoot);
        category ??= CategoryForSource(source);
        if (!string.IsNullOrWhiteSpace(category.SourceRoot))
            return Path.Combine(categoryRoot,
                Path.GetRelativePath(category.SourceRoot, source));

        var sourceRoot = Path.GetPathRoot(source)!;
        var driveLabel = sourceRoot
            .TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar)
            .Replace(":", "", StringComparison.Ordinal);
        return Path.Combine(categoryRoot, driveLabel,
            Path.GetRelativePath(sourceRoot, source));
    }

    private static bool PathEquals(string left, string right) =>
        string.Equals(Path.TrimEndingDirectorySeparator(Path.GetFullPath(left)),
            Path.TrimEndingDirectorySeparator(Path.GetFullPath(right)),
            StringComparison.OrdinalIgnoreCase);
}

internal static class AppStorage
{
    public static readonly string Root = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".dirmover");

    public static void EnsureExists()
    {
        Directory.CreateDirectory(Root);
        File.SetAttributes(Root, File.GetAttributes(Root) | FileAttributes.Hidden);
        File.Delete(Path.Combine(Root, "initial-scan.completed"));
    }
}

internal static class SelfTest
{
    public static void Run()
    {
        var root = Path.GetPathRoot(Environment.SystemDirectory)!;
        var parent = Path.Combine(root, "one");
        Check(MoveEngine.SameOrInside(Path.Combine(parent, "two"), parent));
        Check(MoveEngine.SameOrInside(parent, parent));
        Check(!MoveEngine.SameOrInside(Path.Combine(root, "other"), parent));
        Check(MainForm.SizeText(1024) == "1 KB");
        var runs = MftScanner.DecodeRuns([0x11, 0x03, 0x05, 0x11, 0x02, 0xFE, 0x00]);
        Check(runs.Count == 2 && runs[0].LogicalCluster == 5 && runs[0].ClusterCount == 3);
        Check(runs[1].VirtualCluster == 3 && runs[1].LogicalCluster == 3);
        Check(MftScanner.ReparseDirectorySelfTest());
        var planned = StoragePlanner.DestinationUnderBase(
            @"Z:\DirMoverData",
            Path.Combine(Environment.GetFolderPath(
                Environment.SpecialFolder.LocalApplicationData), "Vendor"));
        Check(planned.Equals(
            Path.Combine(@"Z:\DirMoverData", "AppData", "Local", "Vendor"),
            StringComparison.OrdinalIgnoreCase));
        var localLow = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
            "AppData", "LocalLow", "Vendor");
        Check(StoragePlanner.CategoryForSource(localLow).Key == "LocalLow");
        Check(StoragePlanner.DestinationUnderCategory(
                @"Z:\LocalLow", localLow)
            .Equals(Path.Combine(@"Z:\LocalLow", "Vendor"),
                StringComparison.OrdinalIgnoreCase));
        var link = new LinkRow("目录链接", @"C:\link", @"D:\target", "有效", @"C:\", DateTime.UtcNow);
        var restoredLink = JsonSerializer.Deserialize<LinkRow>(JsonSerializer.Serialize(link));
        Check(restoredLink?.Path == link.Path && restoredLink.ScanRoot == link.ScanRoot);
        Console.WriteLine("Self-test passed.");
    }

    private static void Check(bool condition)
    {
        if (!condition) throw new InvalidOperationException("Self-test failed.");
    }
}

internal sealed record FolderRow(
    string Name, string FullPath, long Bytes, long AllocatedBytes, long FileCount, long LinkCount,
    long ErrorCount, bool IsLink, string Note, bool IsMeasured = true)
{
    public string SizeText => IsMeasured ? MainForm.SizeText(Bytes) : "未扫描";
    public string AllocatedText => IsMeasured ? MainForm.SizeText(AllocatedBytes) : "未扫描";
    public string FileCountText => IsMeasured ? FileCount.ToString("N0") : "—";
    public string LinkCountText => IsLink ? "1" : IsMeasured ? LinkCount.ToString("N0") : "—";
    public string DisplayStatus => IsLink ? "目录链接" :
        !IsMeasured ? ErrorCount > 0 ? "属性不可读" : "未扫描" :
        ErrorCount > 0 ? $"{ErrorCount:N0} 项不可读" : "已扫描";
    public string? LinkTargetTooltip
    {
        get
        {
            if (!IsLink) return null;
            var arrow = Note.IndexOf('→');
            return arrow >= 0 ? $"链接目标：{Note[(arrow + 1)..].Trim()}" : "链接目标不可读";
        }
    }
}

internal sealed record FolderStats(long Bytes, long Files, long Links, long Errors);
internal sealed record ScanProgress(int Percentage, string Message);
internal sealed record ScanSnapshot(List<FolderRow> Folders, List<LinkRow> Links, string Engine);
internal sealed class AppSettings
{
    public string? ManagedRoot { get; set; }
    public Dictionary<string, string> CategoryRoots { get; set; } = [];
}

internal sealed record ManagedCategoryDefinition(
    string Key, string Label, string? SourceRoot, string DefaultRelativeRoot);

internal sealed record ManagedCategoryRow(
    string Key, string Label, string Path, bool IsOverridden)
{
    public string ModeText => IsOverridden ? "独立设置" : "跟随统一根目录";
}

internal sealed record LinkRow(
    string Kind, string Path, string Target, string State,
    string ScanRoot = "", DateTime DetectedAtUtc = default)
{
    public string DetectedText => DetectedAtUtc == default
        ? "" : DetectedAtUtc.ToLocalTime().ToString("yyyy-MM-dd HH:mm");
}
internal sealed record ProcessResult(int ExitCode, string Output, string Error);

internal sealed class MoveRecord
{
    public Guid Id { get; set; }
    public string Source { get; set; } = "";
    public string Destination { get; set; } = "";
    public string? BackupPath { get; set; }
    public DateTime CreatedAtUtc { get; set; }
    public string Stage { get; set; } = "";
}

internal sealed class MoveRow
{
    public MoveRow(MoveRecord record, string status)
    {
        Id = record.Id;
        Source = record.Source;
        Destination = record.Destination;
        Status = status;
        Created = record.CreatedAtUtc.ToLocalTime().ToString("yyyy-MM-dd HH:mm");
    }

    public Guid Id { get; }
    public string Status { get; }
    public string Source { get; }
    public string Destination { get; }
    public string Created { get; }
}
