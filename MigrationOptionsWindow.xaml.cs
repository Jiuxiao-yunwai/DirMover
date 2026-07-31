using Microsoft.Win32;
using System.IO;
using System.Windows;

namespace DirMover;

public partial class MigrationOptionsWindow : Window
{
    private readonly string source;
    private readonly string? managedRoot;

    public string? SelectedDestination { get; private set; }

    public MigrationOptionsWindow(string source)
    {
        this.source = Path.GetFullPath(source);
        managedRoot = StoragePlanner.CategoryRootForSource(this.source);
        InitializeComponent();

        SourceText.Text = $"待迁移：{this.source}";
        var category = StoragePlanner.CategoryForSource(this.source);
        ManagedPathText.Text = managedRoot is null
            ? "没有可用的统一托管位置"
            : $"{category.Label}：{managedRoot}";
        SingleDriveWarning.Visibility = StoragePlanner.HasOnlySystemDrive()
            ? Visibility.Visible : Visibility.Collapsed;

        var canUseManaged = StoragePlanner.CanUseForSource(
            managedRoot, this.source, out var reason);
        ManagedOption.IsEnabled = canUseManaged;
        ManagedReasonText.Text = reason;
        if (canUseManaged)
            ManagedOption.IsChecked = true;
        else
            CustomOption.IsChecked = true;
        UpdateDestinationPreview();
    }

    private void DestinationMode_Changed(object sender, RoutedEventArgs e)
    {
        if (!IsInitialized) return;
        UpdateDestinationPreview();
    }

    private void BrowseCustom_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFolderDialog
        {
            Title = "选择本次迁移的托管根目录",
            Multiselect = false
        };
        if (dialog.ShowDialog(this) != true) return;
        CustomPathText.Text = dialog.FolderName;
        CustomOption.IsChecked = true;
        UpdateDestinationPreview();
    }

    private void UpdateDestinationPreview()
    {
        if (ManagedOption.IsChecked == true)
        {
            DestinationPreviewText.Text = managedRoot is null
                ? "没有可用的统一托管位置"
                : StoragePlanner.DestinationUnderCategory(managedRoot, source);
            return;
        }
        DestinationPreviewText.Text = string.IsNullOrWhiteSpace(CustomPathText.Text)
            ? "请选择一个位于其他 NTFS 分区的目录"
            : StoragePlanner.DestinationUnderBase(CustomPathText.Text, source);
    }

    private void Confirm_Click(object sender, RoutedEventArgs e)
    {
        var root = ManagedOption.IsChecked == true
            ? managedRoot : CustomPathText.Text;
        if (!StoragePlanner.CanUseForSource(root, source, out var reason))
        {
            System.Windows.MessageBox.Show(this, reason, "迁移位置不可用",
                MessageBoxButton.OK, MessageBoxImage.Warning);
            return;
        }

        SelectedDestination = ManagedOption.IsChecked == true
            ? StoragePlanner.DestinationUnderCategory(root!, source)
            : StoragePlanner.DestinationUnderBase(root!, source);
        DialogResult = true;
    }
}
