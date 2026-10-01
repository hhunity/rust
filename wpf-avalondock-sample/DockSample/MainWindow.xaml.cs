using System.Windows;
using DockSample.Docking;
using DockSample.ViewModels;

namespace DockSample;

public partial class MainWindow : Window
{
    // コンストラクタインジェクション
    public MainWindow(MainViewModel viewModel, DockLayoutStore layoutStore)
    {
        InitializeComponent();
        DataContext = viewModel;

        // レイアウトの復元・保存は DockingManager(View) が必要なので View 側で行う
        Loaded  += (_, _) => layoutStore.Load(DockManager, viewModel.Tools);
        Closing += (_, _) => layoutStore.Save(DockManager);
    }
}
