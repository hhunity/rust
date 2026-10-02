using CommunityToolkit.Mvvm.ComponentModel;

namespace DockSample.ViewModels;

/// <summary>中央のドキュメント領域に表示するペイン</summary>
public partial class DocumentViewModel : PaneViewModel
{
    public DocumentViewModel(string contentId, string title, string? buttonText = null)
        : base(contentId, title, DockLocation.Document, buttonText)
    {
    }

    /// <summary>非表示にしても ViewModel は残るので、再表示すると内容もそのまま</summary>
    [ObservableProperty] private string _text = "";
}
