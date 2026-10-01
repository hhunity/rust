using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;

namespace DockSample.ViewModels;

public partial class MainViewModel : ObservableObject
{
    /// <param name="panes">DI に PaneViewModel として登録したものが全部（登録順で）入ってくる</param>
    public MainViewModel(IEnumerable<PaneViewModel> panes)
    {
        // ペインは最初から全部登録しておき、表示/非表示は IsVisible で切り替える。
        // （コレクションから消さないので、AvalonDock が隠す直前の位置を覚えていられる）
        Panes = new ObservableCollection<PaneViewModel>(panes);
    }

    /// <summary>上部のトグルボタン と DockingManager.AnchorablesSource の両方のバインド先</summary>
    public ObservableCollection<PaneViewModel> Panes { get; }
}
