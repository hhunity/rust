using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using DockSample.Services;

namespace DockSample.ViewModels;

public partial class MainViewModel : ObservableObject
{
    private readonly Func<DocumentViewModel> _documentFactory;
    private readonly ILogService _log;

    public MainViewModel(
        ExplorerViewModel explorer,
        PropertiesViewModel properties,
        OutputViewModel output,
        Func<DocumentViewModel> documentFactory,
        ILogService log)
    {
        Explorer = explorer;
        Properties = properties;
        Output = output;
        _documentFactory = documentFactory;
        _log = log;

        // ツールは最初から全部登録しておき、表示/非表示は IsVisible で切り替える。
        // （コレクションから消さないので、AvalonDock が隠す直前の位置を覚えていられる）
        Tools = new() { explorer, properties, output };
    }

    // ToggleButton のバインド先
    public ExplorerViewModel Explorer { get; }
    public PropertiesViewModel Properties { get; }
    public OutputViewModel Output { get; }

    /// <summary>DockingManager.DocumentsSource</summary>
    public ObservableCollection<DocumentViewModel> Documents { get; } = new();

    /// <summary>DockingManager.AnchorablesSource</summary>
    public ObservableCollection<ToolViewModel> Tools { get; }

    [ObservableProperty] private object? _activeContent;

    [RelayCommand]
    private void NewDocument()
    {
        var doc = _documentFactory();
        doc.CloseRequested += OnDocumentCloseRequested;
        Documents.Add(doc);
        ActiveContent = doc;
    }

    private void OnDocumentCloseRequested(object? sender, EventArgs e)
    {
        if (sender is not DocumentViewModel doc) return;
        doc.CloseRequested -= OnDocumentCloseRequested;
        Documents.Remove(doc);
        _log.Write($"{doc.Title} を閉じました");
    }
}
