using System.Windows;
using DockSample.ViewModels;

namespace DockSample;

public partial class MainWindow : Window
{
    // コンストラクタインジェクション
    public MainWindow(MainViewModel viewModel)
    {
        InitializeComponent();
        DataContext = viewModel;
    }
}
