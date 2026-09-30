using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using DockSample.Services;

namespace DockSample.ViewModels;

public partial class MainViewModel : ObservableObject
{
    private readonly ExplorerViewModel _explorer;
    private readonly PropertiesViewModel _properties;
    private readonly OutputViewModel _output;
    private readonly Func<DocumentViewModel> _documentFactory;
    private readonly ILogService _log;

    public MainViewModel(
        ExplorerViewModel explorer,
        PropertiesViewModel properties,
        OutputViewModel output,
        Func<DocumentViewModel> documentFactory,
        ILogService log)
    {
        _explorer = explorer;
        _properties = properties;
        _output = output;
        _documentFactory = documentFactory;
        _log = log;
    }

    /// <summary>DockingManager.DocumentsSource</summary>
    public ObservableCollection<DocumentViewModel> Documents { get; } = new();

    /// <summary>DockingManager.AnchorablesSource</summary>
    public ObservableCollection<ToolViewModel> Tools { get; } = new();

    [ObservableProperty] private object? _activeContent;

    // ===== ボタンのコマンド =====
    [RelayCommand]
    private void NewDocument()
    {
        var doc = _documentFactory();
        doc.CloseRequested += OnPaneCloseRequested;
        Documents.Add(doc);
        ActiveContent = doc;
    }

    [RelayCommand] private void ShowExplorer()   => ShowTool(_explorer);
    [RelayCommand] private void ShowProperties() => ShowTool(_properties);
    [RelayCommand] private void ShowOutput()     => ShowTool(_output);

    private void ShowTool(ToolViewModel tool)
    {
        if (!Tools.Contains(tool))
        {
            tool.CloseRequested += OnPaneCloseRequested;
            Tools.Add(tool);   // → LayoutInitializer が配置先を決める
            _log.Write($"{tool.Title} を表示");
        }
        tool.IsSelected = true;
        ActiveContent = tool;
    }

    // ===== ×ボタンで閉じられたとき =====
    private void OnPaneCloseRequested(object? sender, EventArgs e)
    {
        if (sender is not PaneViewModel pane) return;
        pane.CloseRequested -= OnPaneCloseRequested;

        switch (pane)
        {
            case DocumentViewModel doc: Documents.Remove(doc); break;
            case ToolViewModel tool:    Tools.Remove(tool);    break;
        }
        _log.Write($"{pane.Title} を閉じました");
    }
}
