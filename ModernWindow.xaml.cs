using Microsoft.Win32;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Media;
using WpfBinding = System.Windows.Data.Binding;
using WpfClipboard = System.Windows.Clipboard;
using WpfCursors = System.Windows.Input.Cursors;
using WpfDataGrid = System.Windows.Controls.DataGrid;
using WpfKeyEventArgs = System.Windows.Input.KeyEventArgs;
using WpfMessageBox = System.Windows.MessageBox;

namespace DirMover;

public partial class ModernWindow : Window
{
    private readonly ObservableCollection<FolderRow> folders = [];
    private readonly ObservableCollection<MoveRow> moves = [];
    private readonly ObservableCollection<LinkRow> links = [];
    private readonly ObservableCollection<ManagedCategoryRow> managedLocations = [];
    private readonly Dictionary<string, ScanSnapshot> scanCache =
        new(StringComparer.OrdinalIgnoreCase);
    private readonly string? startupScan;
    private CancellationTokenSource? scanCancellation;
    private CancellationTokenSource? previewCancellation;
    private int previewVersion;
    private bool showingPreview;

    public ModernWindow(string? startupScan = null)
    {
        this.startupScan = startupScan;
        InitializeComponent();

        FolderGrid.ItemsSource = folders;
        MoveGrid.ItemsSource = moves;
        LinkGrid.ItemsSource = links;
        ManagedLocationList.ItemsSource = managedLocations;
        RootPathText.Text = startupScan ??
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        PrivilegeText.Text = Program.IsAdministrator() ? "管理员权限" : "普通权限";
        Loaded += Window_Loaded;
    }

    private async void Window_Loaded(object sender, RoutedEventArgs e)
    {
        RefreshMoves();
        RefreshLinks();
        RefreshSettings();
        if (StoragePlanner.HasOnlySystemDrive())
        {
            Show("当前只检测到系统盘 C:。目录迁移到同一分区并不能释放空间，" +
                 "因此统一托管已停用；只有连接其他 NTFS 磁盘后才能使用自定义迁移。",
                "未检测到其他磁盘", MessageBoxImage.Information);
        }
        await ShowRootContentsAsync(RootPathText.Text);
        if (startupScan is not null)
            await ScanAsync(false);
    }

    private void SpaceNav_Checked(object sender, RoutedEventArgs e) =>
        Navigate(SpaceView, "空间分析", "快速查看目录占用并选择需要迁移的内容");

    private void MovesNav_Checked(object sender, RoutedEventArgs e) =>
        Navigate(MovesView, "迁移记录", "启动时自动核对原位置、目标位置和目录联接");

    private void LinksNav_Checked(object sender, RoutedEventArgs e) =>
        Navigate(LinksView, "链接检测", "查找目录联接和符号链接，不跟随链接目标");

    private void SettingsNav_Checked(object sender, RoutedEventArgs e)
    {
        RefreshSettings();
        Navigate(SettingsView, "设置", "管理所有迁移文件夹的默认托管位置");
    }

    private void Navigate(UIElement view, string title, string subtitle)
    {
        if (!IsLoaded) return;
        SpaceView.Visibility = Visibility.Collapsed;
        MovesView.Visibility = Visibility.Collapsed;
        LinksView.Visibility = Visibility.Collapsed;
        SettingsView.Visibility = Visibility.Collapsed;
        view.Visibility = Visibility.Visible;
        PageTitle.Text = title;
        PageSubtitle.Text = subtitle;
    }

    private async void PresetBox_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!IsLoaded) return;
        var selected = PresetBox.SelectedIndex switch
        {
            0 => Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            1 => Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            2 => Path.Combine(Environment.GetFolderPath(
                Environment.SpecialFolder.UserProfile), "AppData", "LocalLow"),
            3 => Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),
            4 => Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86),
            _ => PickFolder("选择要分析的目录") ?? RootPathText.Text
        };
        RootPathText.Text = selected;
        await ShowRootContentsAsync(selected);
    }

    private async void Scan_Click(object sender, RoutedEventArgs e) => await ScanAsync();
    private async void RefreshScan_Click(object sender, RoutedEventArgs e) =>
        await ScanAsync(force: true);
    private void Stop_Click(object sender, RoutedEventArgs e) => scanCancellation?.Cancel();
    private async void Move_Click(object sender, RoutedEventArgs e) => await MoveSelectedAsync();
    private void RefreshMoves_Click(object sender, RoutedEventArgs e) => RefreshMoves();
    private async void Restore_Click(object sender, RoutedEventArgs e) => await RestoreSelectedAsync();
    private void ChooseManagedRoot_Click(object sender, RoutedEventArgs e) => ChooseManagedRoot();
    private void UseAutomaticRoot_Click(object sender, RoutedEventArgs e)
    {
        SettingsStore.Save(new AppSettings());
        RefreshSettings();
        Busy("已恢复自动选择托管位置", false);
    }
    private void OpenManagedRoot_Click(object sender, RoutedEventArgs e) => OpenManagedRoot();
    private void ChooseCategoryRoot_Click(object sender, RoutedEventArgs e)
    {
        if (sender is System.Windows.Controls.Button { Tag: string categoryKey })
            ChooseCategoryRoot(categoryKey);
    }
    private void ResetCategoryRoots_Click(object sender, RoutedEventArgs e)
    {
        var settings = SettingsStore.Load();
        settings.CategoryRoots.Clear();
        SettingsStore.Save(settings);
        RefreshSettings();
        Busy("已清除各来源的独立位置", false);
    }

    private void OpenStore_Click(object sender, RoutedEventArgs e) =>
        Process.Start(new ProcessStartInfo("explorer.exe", $"/select,\"{MoveStore.FilePath}\"")
        {
            UseShellExecute = true
        });

    private async void DetectLinks_Click(object sender, RoutedEventArgs e)
    {
        var root = PickFolder("选择要递归检测软链接或目录联接的位置");
        if (root is not null)
            await DetectLinksAsync(root);
    }

    private void Grid_Sorting(object sender, DataGridSortingEventArgs e)
    {
        // WPF's virtualized collection view sorts without rebuilding rows.
        var property = e.Column.SortMemberPath;
        if (string.IsNullOrEmpty(property) && e.Column is DataGridBoundColumn bound &&
            bound.Binding is WpfBinding binding)
            property = binding.Path.Path;
        if (string.IsNullOrEmpty(property)) return;
        var view = CollectionViewSource.GetDefaultView(FolderGrid.ItemsSource);
        view.SortDescriptions.Clear();
        var direction = e.Column.SortDirection != ListSortDirection.Descending
            ? ListSortDirection.Descending : ListSortDirection.Ascending;
        view.SortDescriptions.Add(new SortDescription(property, direction));
        e.Column.SortDirection = direction;
        e.Handled = true;
    }

    private async Task<bool> ScanAsync(bool requestElevation = true, bool force = false)
    {
        if (!Directory.Exists(RootPathText.Text))
        {
            Show("扫描目录不存在。", "DirMover", MessageBoxImage.Warning);
            return false;
        }
        var root = Path.GetFullPath(RootPathText.Text);
        previewCancellation?.Cancel();
        previewVersion++;
        if (!force && scanCache.TryGetValue(root, out var cached))
        {
            ApplySnapshot(cached);
            Busy($"已从本次运行缓存加载：{cached.Folders.Count:N0} 个文件夹", false);
            return true;
        }
        if (requestElevation && !Program.IsAdministrator())
        {
            if (Program.TryRestartElevated("--scan", RootPathText.Text))
                Close();
            return false;
        }

        scanCancellation?.Cancel();
        var activeScan = new CancellationTokenSource();
        scanCancellation = activeScan;
        Busy($"正在扫描 {root}…", true, false);
        var progress = new Progress<ScanProgress>(value =>
        {
            if (!activeScan.IsCancellationRequested && IsCurrentRoot(root))
                UpdateScanProgress(value);
        });

        try
        {
            var snapshot = await Scanner.ScanChildrenAsync(
                root, progress, activeScan.Token);
            scanCache[root] = snapshot;
            LinkStore.ReplaceForRoot(root, snapshot.Links);
            RefreshLinks();
            if (!IsCurrentRoot(root))
                return true;
            ApplySnapshot(snapshot);
            Busy($"扫描完成（{snapshot.Engine}）：{snapshot.Folders.Count:N0} 个文件夹，" +
                 $"发现 {snapshot.Links.Count:N0} 个直接链接", false);
            return true;
        }
        catch (OperationCanceledException)
        {
            if (IsCurrentRoot(root))
                Busy("扫描已停止，仍显示扫描前的目录列表", false);
            return false;
        }
        catch (Exception ex)
        {
            if (IsCurrentRoot(root))
            {
                Busy("扫描失败，仍显示扫描前的目录列表", false);
                Show(ex.Message, "扫描失败", MessageBoxImage.Error);
            }
            return false;
        }
    }

    private async Task ShowRootContentsAsync(string selectedPath)
    {
        scanCancellation?.Cancel();
        previewCancellation?.Cancel();
        var version = ++previewVersion;

        if (!Directory.Exists(selectedPath))
        {
            folders.Clear();
            showingPreview = true;
            UpdateStats();
            Busy("目录不存在", false);
            return;
        }

        var root = Path.GetFullPath(selectedPath);
        if (scanCache.TryGetValue(root, out var cached))
        {
            ApplySnapshot(cached);
            Busy($"已从本次运行缓存加载：{cached.Folders.Count:N0} 个文件夹", false);
            return;
        }

        folders.Clear();
        showingPreview = true;
        UpdateStats();
        Busy($"正在列出 {root} 的一级文件夹…", false);
        previewCancellation = new CancellationTokenSource();

        try
        {
            var preview = await Scanner.ListChildFoldersAsync(
                root, previewCancellation.Token);
            if (version != previewVersion || !IsCurrentRoot(root))
                return;
            ApplyPreview(preview);
            Busy($"已列出 {preview.Count:N0} 个一级文件夹；点击“扫描”计算大小", false);
        }
        catch (OperationCanceledException) { }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            if (version != previewVersion || !IsCurrentRoot(root))
                return;
            Busy("无法列出该目录", false);
            Show(ex.Message, "读取目录失败", MessageBoxImage.Warning);
        }
    }

    private bool IsCurrentRoot(string root) =>
        Directory.Exists(RootPathText.Text) &&
        string.Equals(
            Path.TrimEndingDirectorySeparator(Path.GetFullPath(RootPathText.Text)),
            Path.TrimEndingDirectorySeparator(root),
            StringComparison.OrdinalIgnoreCase);

    private async Task MoveSelectedAsync()
    {
        if (FolderGrid.SelectedItem is not FolderRow row)
        {
            Show("请先选择一个文件夹。");
            return;
        }
        if (!Program.IsAdministrator())
        {
            if (Program.TryRestartElevated("--scan", RootPathText.Text))
                Close();
            return;
        }
        if (row.IsLink)
        {
            Show("选中项已经是链接，不能再次迁移。");
            return;
        }
        if (!row.IsMeasured)
        {
            Show("该目录尚未扫描。请先点击“扫描”，完成大小和链接安全检查后再迁移。",
                "需要先扫描", MessageBoxImage.Information);
            return;
        }
        if (row.LinkCount > 0)
        {
            Show("此文件夹内部含有目录联接或符号链接。为避免遗漏数据，DirMover 不会迁移它。",
                "安全检查未通过", MessageBoxImage.Warning);
            return;
        }

        var options = new MigrationOptionsWindow(row.FullPath) { Owner = this };
        if (options.ShowDialog() != true ||
            string.IsNullOrWhiteSpace(options.SelectedDestination))
            return;
        var destination = options.SelectedDestination!;
        if (WpfMessageBox.Show(this,
                $"将要迁移：\n{row.FullPath}\n\n到：\n{destination}\n\n请先关闭正在使用此目录的程序。",
                "确认迁移", MessageBoxButton.OKCancel, MessageBoxImage.Warning) != MessageBoxResult.OK)
            return;

        Busy("正在安全复制、校验并创建目录联接，请勿关闭程序…", true);
        try
        {
            var result = await MoveEngine.MoveAsync(row.FullPath, destination);
            scanCache.Clear();
            RefreshMoves();
            Busy("迁移完成", false);
            Show(result, "迁移完成", MessageBoxImage.Information);
            await ScanAsync(false, true);
        }
        catch (Exception ex)
        {
            RefreshMoves();
            Busy("迁移未完成", false);
            Show(ex.Message, "迁移失败", MessageBoxImage.Error);
        }
    }

    private async Task RestoreSelectedAsync()
    {
        if (MoveGrid.SelectedItem is not MoveRow row) return;
        if (!Program.IsAdministrator())
        {
            Show("还原需要管理员权限。确认 UAC 后，请重新选择迁移记录。",
                "DirMover", MessageBoxImage.Information);
            if (Program.TryRestartElevated())
                Close();
            return;
        }
        var record = MoveStore.Load().FirstOrDefault(x => x.Id == row.Id);
        if (record is null) return;
        if (!MoveEngine.IsActive(record))
        {
            Show("该记录目前不是有效链接，不能自动还原。数据不会被改动。",
                "DirMover", MessageBoxImage.Warning);
            return;
        }
        if (WpfMessageBox.Show(this, $"把数据还原到：\n{record.Source}\n\n还原需要原磁盘有足够空间。",
                "确认还原", MessageBoxButton.OKCancel, MessageBoxImage.Warning) != MessageBoxResult.OK)
            return;

        Busy("正在复制回原磁盘并校验…", true);
        try
        {
            var message = await MoveEngine.RestoreAsync(record);
            scanCache.Clear();
            RefreshMoves();
            Busy("还原完成", false);
            Show(message, "还原完成", MessageBoxImage.Information);
        }
        catch (Exception ex)
        {
            RefreshMoves();
            Busy("还原未完成", false);
            Show(ex.Message, "还原失败", MessageBoxImage.Error);
        }
    }

    private async Task DetectLinksAsync(string root)
    {
        links.Clear();
        Busy($"正在检测 {root}…", true);
        try
        {
            var found = await Scanner.FindLinksAsync(root, CancellationToken.None);
            LinkStore.ReplaceForRoot(root, found);
            RefreshLinks();
            Busy($"检测完成：在所选目录的直接子项中找到 {found.Count:N0} 个链接", false);
        }
        catch (Exception ex)
        {
            Busy("检测失败", false);
            Show(ex.Message, "检测失败", MessageBoxImage.Error);
        }
    }

    private void RefreshMoves()
    {
        moves.Clear();
        foreach (var record in MoveStore.Load().OrderByDescending(x => x.CreatedAtUtc))
            moves.Add(new MoveRow(record, MoveEngine.StatusOf(record)));
        StatusText.Text = moves.Count == 0 ? "没有迁移记录" : $"已检查 {moves.Count:N0} 条迁移记录";
    }

    private void RefreshLinks()
    {
        links.Clear();
        foreach (var link in LinkStore.Load().OrderByDescending(x => x.DetectedAtUtc))
            links.Add(link);
    }

    private void RefreshSettings()
    {
        var settings = SettingsStore.Load();
        var drives = StoragePlanner.FixedNtfsDrives();
        managedLocations.Clear();
        foreach (var category in StoragePlanner.Categories)
        {
            var path = StoragePlanner.CategoryRoot(category.Key) ??
                       "不可用（没有其他固定 NTFS 分区）";
            managedLocations.Add(new ManagedCategoryRow(
                category.Key, category.Label, path,
                settings.CategoryRoots.ContainsKey(category.Key)));
        }
        ManagedModeText.Text = settings.CategoryRoots.Count > 0
            ? $"{settings.CategoryRoots.Count:N0} 项独立配置"
            : string.IsNullOrWhiteSpace(settings.ManagedRoot) ? "自动选择" : "统一根目录";
        DriveSummaryText.Text = drives.Count == 0
            ? "未检测到可用的固定 NTFS 分区"
            : $"检测到 {drives.Count:N0} 个固定 NTFS 分区：{string.Join("、", drives.Select(x => x.Name.TrimEnd('\\')))}";
        SettingsWarning.Visibility = StoragePlanner.HasOnlySystemDrive()
            ? Visibility.Visible : Visibility.Collapsed;
    }

    private void ChooseManagedRoot()
    {
        var selected = PickFolder("选择全局托管根目录");
        if (selected is null) return;
        if (!StoragePlanner.IsValidManagedRoot(selected, out var reason))
        {
            Show(reason, "位置不可用", MessageBoxImage.Warning);
            return;
        }
        SettingsStore.Save(new AppSettings { ManagedRoot = Path.GetFullPath(selected) });
        RefreshSettings();
        Busy("全局托管位置已更新", false);
    }

    private void ChooseCategoryRoot(string categoryKey)
    {
        var category = StoragePlanner.Categories.First(
            x => x.Key.Equals(categoryKey, StringComparison.OrdinalIgnoreCase));
        var selected = PickFolder($"选择 {category.Label} 的托管位置");
        if (selected is null) return;
        if (!StoragePlanner.IsValidManagedRoot(selected, out var reason))
        {
            Show(reason, "位置不可用", MessageBoxImage.Warning);
            return;
        }
        var settings = SettingsStore.Load();
        settings.CategoryRoots[category.Key] = Path.GetFullPath(selected);
        SettingsStore.Save(settings);
        RefreshSettings();
        Busy($"{category.Label} 的托管位置已更新", false);
    }

    private void OpenManagedRoot()
    {
        var root = StoragePlanner.EffectiveManagedRoot();
        if (!StoragePlanner.IsValidManagedRoot(root, out var reason))
        {
            Show(reason, "位置不可用", MessageBoxImage.Warning);
            return;
        }
        try
        {
            Directory.CreateDirectory(root!);
            Process.Start(new ProcessStartInfo("explorer.exe", $"\"{root}\"")
                { UseShellExecute = true });
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            Show(ex.Message, "无法打开托管位置", MessageBoxImage.Error);
        }
    }

    private void ApplySnapshot(ScanSnapshot snapshot)
    {
        folders.Clear();
        showingPreview = false;
        foreach (var row in snapshot.Folders.OrderByDescending(x => x.AllocatedBytes))
            folders.Add(row);
        UpdateStats();
    }

    private void ApplyPreview(IEnumerable<FolderRow> preview)
    {
        folders.Clear();
        showingPreview = true;
        foreach (var row in preview.OrderBy(x => x.Name, StringComparer.CurrentCultureIgnoreCase))
            folders.Add(row);
        UpdateStats();
    }

    private void UpdateStats()
    {
        FolderCountText.Text = folders.Count.ToString("N0");
        LogicalSizeText.Text = showingPreview
            ? "未扫描" : MainForm.SizeText(folders.Sum(x => x.Bytes));
        AllocatedSizeText.Text = showingPreview
            ? "未扫描" : MainForm.SizeText(folders.Sum(x => x.AllocatedBytes));
    }

    private void Busy(string message, bool busy, bool indeterminate = true)
    {
        StatusText.Text = message;
        ProgressIndicator.Visibility = busy ? Visibility.Visible : Visibility.Collapsed;
        ProgressIndicator.IsIndeterminate = busy && indeterminate;
        ScanProgressPanel.Visibility = busy ? Visibility.Visible : Visibility.Collapsed;
        ScanProgressBar.IsIndeterminate = busy && indeterminate;
        if (busy && indeterminate)
        {
            ScanProgressText.Text = message;
            ScanPercentText.Text = "…";
        }
        Mouse.OverrideCursor = busy ? WpfCursors.Wait : null;
    }

    private void UpdateScanProgress(ScanProgress progress)
    {
        var value = Math.Clamp(progress.Percentage, 0, 100);
        ScanProgressPanel.Visibility = Visibility.Visible;
        ScanProgressBar.IsIndeterminate = false;
        ScanProgressBar.Value = value;
        ProgressIndicator.IsIndeterminate = false;
        ProgressIndicator.Value = value;
        ScanProgressText.Text = progress.Message;
        ScanPercentText.Text = $"{value}%";
        StatusText.Text = progress.Message;
    }

    private void Grid_PreviewMouseRightButtonDown(object sender, MouseButtonEventArgs e)
    {
        if (sender is not WpfDataGrid grid || e.OriginalSource is not DependencyObject source) return;
        var row = FindVisualParent<DataGridRow>(source);
        if (row is null) return;
        row.IsSelected = true;
        grid.SelectedItem = row.Item;
    }

    private void Grid_PreviewKeyDown(object sender, WpfKeyEventArgs e)
    {
        if (e.Key != Key.C || !Keyboard.Modifiers.HasFlag(ModifierKeys.Control) ||
            sender is not WpfDataGrid grid) return;
        CopyItem(grid.SelectedItem);
        e.Handled = true;
    }

    private void OpenSelected_Click(object sender, RoutedEventArgs e)
    {
        var item = ContextGrid(sender)?.SelectedItem;
        var path = item switch
        {
            FolderRow folder => folder.FullPath,
            MoveRow move => Directory.Exists(move.Source) ? move.Source : move.Destination,
            LinkRow link => link.Path,
            _ => null
        };
        if (string.IsNullOrWhiteSpace(path)) return;
        var arguments = item is FolderRow && Directory.Exists(path)
            ? $"\"{path}\"" : $"/select,\"{path}\"";
        Process.Start(new ProcessStartInfo("explorer.exe", arguments) { UseShellExecute = true });
    }

    private void CopySelected_Click(object sender, RoutedEventArgs e) =>
        CopyItem(ContextGrid(sender)?.SelectedItem);

    private static void CopyItem(object? item)
    {
        var text = item switch
        {
            FolderRow folder => folder.FullPath,
            MoveRow move => $"原位置：{move.Source}{Environment.NewLine}新位置：{move.Destination}",
            LinkRow link => $"链接：{link.Path}{Environment.NewLine}目标：{link.Target}",
            _ => null
        };
        if (!string.IsNullOrEmpty(text))
            WpfClipboard.SetText(text);
    }

    private static WpfDataGrid? ContextGrid(object sender)
    {
        if (sender is not System.Windows.Controls.MenuItem item) return null;
        var menu = ItemsControl.ItemsControlFromItemContainer(item) as
            System.Windows.Controls.ContextMenu;
        return menu?.PlacementTarget as WpfDataGrid;
    }

    private static T? FindVisualParent<T>(DependencyObject child) where T : DependencyObject
    {
        for (var current = child; current is not null; current = VisualTreeHelper.GetParent(current))
            if (current is T match) return match;
        return null;
    }

    private string? PickFolder(string title)
    {
        var dialog = new OpenFolderDialog { Title = title, Multiselect = false };
        return dialog.ShowDialog(this) == true ? dialog.FolderName : null;
    }

    private void Show(string message, string title = "DirMover",
        MessageBoxImage image = MessageBoxImage.Information) =>
        WpfMessageBox.Show(this, message, title, MessageBoxButton.OK, image);
}
