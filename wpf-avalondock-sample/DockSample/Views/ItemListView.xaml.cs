using System.Windows.Controls;

namespace DockSample.Views;

public partial class ItemListView : UserControl
{
    public ItemListView()
    {
        InitializeComponent();

        // 追加した行（選ばれた行）が見えるところまでスクロールする（表示だけの処理なので View 側で行う）
        ItemsGrid.SelectionChanged += (_, _) =>
        {
            if (ItemsGrid.SelectedItem is { } item) ItemsGrid.ScrollIntoView(item);
        };
    }
}
