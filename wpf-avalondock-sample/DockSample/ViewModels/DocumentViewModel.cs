using CommunityToolkit.Mvvm.ComponentModel;
using DockSample.Services;

namespace DockSample.ViewModels;

public partial class DocumentViewModel : PaneViewModel
{
    private static int _counter;

    public DocumentViewModel(ILogService log)
        : base($"Document_{Guid.NewGuid():N}")
    {
        Title = $"Document {++_counter}";
        log.Write($"{Title} を作成");
    }

    [ObservableProperty] private string _text = "";
}
