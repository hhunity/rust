//! # マイコン役が担当する、生TCPでのファイル受信ロジック
//!
//! ここは`mqtt-client`実行ファイルだけが使うモジュールです。
//! パソコン役（送信する側）の`send_file_to`関数は[`crate::controller`]モジュールにあります。
//!
//! MQTTは大きなバイナリデータの配信には向いていないため、**ファイルの中身そのものはMQTTに
//! 乗せず、別途つないだ生のTCPソケットで直接送受信**する方式にしています（MQTTは
//! 「送るよ」「ここで待ってるよ」という信号のやり取りだけに使う、という設計です）。

use std::fs;
use std::io::{self, Read};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::thread;

use rumqttc::{Client, QoS};

use crate::messages::ReceivedMsg;
use crate::seq::{next_seq, SeqCounter};

/// 自分自身のIPアドレス（相手から接続してもらうためのアドレス）を推測する。
///
/// 仕組み: 実際には通信しないUDPソケットを作り、ブローカーのアドレスへ`connect`だけしてみる。
/// すると、OSが「そのアドレス宛てに通信するならこのネットワークインターフェースを使う」と
/// 判断してくれるので、そのローカル側アドレスを覗き見ることで自分のIPが分かる、という小技です
/// （C++でいうBSDソケットAPIの`connect()`＋`getsockname()`の組み合わせと全く同じ原理です。
/// UDPソケットの`connect()`は実際にパケットを送るわけではなく、OSに「相手はこのアドレスだ」と
/// 教えるだけ、という点もC/C++のソケットプログラミングと同じです）。
///
/// （ローカルでの動作確認用の簡易実装。インターネット越しやNAT環境では正しく動かないことがある）
pub fn detect_local_ip(broker_host: &str, broker_port: u16) -> String {
    let socket = UdpSocket::bind("0.0.0.0:0").expect("UDPソケットの作成に失敗しました");
    socket
        .connect((broker_host, broker_port))
        .expect("ブローカーへの疑似接続に失敗しました");
    socket
        .local_addr()
        .expect("ローカルアドレスの取得に失敗しました")
        .ip()
        .to_string()
}

/// 固定ポートのTcpListenerで、来た接続を延々と受け付け続ける（起動時に1回呼ぶ）。
/// 接続が来るたびに別スレッドを立てて受信処理をし、このループ自体は次の接続を待ち続ける
/// （C++でいう、`accept()`を繰り返すループから、接続ごとに`std::thread`を起こす
/// 定番のマルチスレッドサーバーの作り方と同じです）。
/// これにより、何度でも（複数回・複数相手から）ファイルを受け取れる「常時待ち受け」になる。
///
/// client・my_name・received_topic_prefixは、受信結果（成功/失敗）をMQTTで報告するために使う。
/// バルクデータ（ファイルの中身）はMQTTの外（生TCP）でやり取りしているので、この報告が無いと
/// MQTTだけを見ている側からは「実際に届いたかどうか」が一切分からなくなってしまう。
/// 報告先トピックは`<received_topic_prefix>/<my_name>`（例: "chat/file/received/device1"）。
pub fn run_file_listener(
    listener: TcpListener,
    client: Client,
    my_name: String,
    received_topic_prefix: String,
    received_counter: SeqCounter,
) {
    let received_topic = format!("{received_topic_prefix}/{my_name}");
    // listener.incoming() は「接続が来るたびに1つずつ返してくれる、終わりのないイテレータ」。
    // C++の`for (;;) { auto fd = accept(listen_fd, ...); ... }`に相当します。
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[system] 接続の受け付けに失敗しました: {e}");
                continue;
            }
        };
        // 各変数の.clone()は、この後thread::spawnへ「所有権ごと」渡すための複製です。
        // client（内部はArc相当の共有ハンドル）は複製してもコネクション自体は1つを
        // 指し続けるので、C++でいうshared_ptrをコピーする感覚に近いです。
        let client = client.clone();
        let my_name = my_name.clone();
        let received_topic = received_topic.clone();
        let received_counter = received_counter.clone();
        // 1件の受信処理に時間がかかっても次の接続を待てるように、別スレッドに任せる
        thread::spawn(move || {
            match receive_one_file(stream, &my_name, &client, &received_topic, &received_counter) {
                Ok((id, filename, size, path)) => {
                    println!("[system] ジョブ{id}: 受信完了 {filename} ({size} bytes) -> {}", path.display());
                }
                Err(e) => eprintln!("[system] ファイル受信エラー: {e}"),
            }
        });
    }
}

/// すでに繋がっている1本のTCP接続(stream)から、パソコン役が送ってきたファイルを読み取って保存する。
/// 戻り値は (id, 受け取ったファイル名, サイズ, 保存先パス)。
///
/// idが読み取れた後は、途中で失敗しても成功しても、必ずreceived_topicへMQTTで結果を
/// publishする。これでパソコン役（や他の監視者）は、MQTTを見ているだけで生TCP転送の
/// 結果まで分かるようになる。
fn receive_one_file(
    mut stream: TcpStream,
    my_name: &str,
    client: &Client,
    received_topic: &str,
    seq_counter: &SeqCounter,
) -> io::Result<(String, String, u64, PathBuf)> {
    // idの長さ(4バイト)と中身を読み取る
    let mut id_len_buf = [0u8; 4];
    stream.read_exact(&mut id_len_buf)?;
    let id_len = u32::from_be_bytes(id_len_buf) as usize;
    let mut id_buf = vec![0u8; id_len];
    stream.read_exact(&mut id_buf)?;
    let id = String::from_utf8_lossy(&id_buf).to_string();

    // ここから先で失敗したら、idが分かっているのでMQTTへ"failed"を報告してからエラーを返す。
    //
    // `let report = |status: &str, size: u64| { ... };` はクロージャ（無名関数）の定義です。
    // C++のラムダ式`auto report = [&](const char* status, uint64_t size) { ... };`と
    // ほぼ同じ働きで、周囲の変数（id, my_name, client, received_topic, seq_counter）を
    // 自動的に「借用」して使っています（C++ラムダの`[&]`キャプチャに近い挙動です）。
    let result = receive_file_body(&mut stream);
    let report = |status: &str, size: u64| {
        let msg = ReceivedMsg {
            id: id.clone(),
            who: my_name.to_string(),
            status: status.to_string(),
            size,
            seq: next_seq(seq_counter),
        };
        let payload = serde_json::to_vec(&msg).unwrap();
        // publish自体が失敗しても、ここでできることは無いので無視する
        // （`let _ = ...` は、C++でいう「戻り値を(void)キャストして意図的に捨てる」のと同じ）
        let _ = client.publish(received_topic, QoS::AtLeastOnce, false, payload);
    };

    match result {
        Ok((filename, size, save_path)) => {
            report("ok", size);
            Ok((id, filename, size, save_path))
        }
        Err(e) => {
            report("failed", 0);
            Err(e)
        }
    }
}

/// receive_one_fileのうち、「idを読んだ後」のファイル名・サイズ・中身を読み取る部分だけを
/// 切り出した関数。戻り値は (受け取ったファイル名, サイズ, 保存先パス)。
fn receive_file_body(stream: &mut TcpStream) -> io::Result<(String, u64, PathBuf)> {
    // ファイル名の長さ(4バイト)を読み取る
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let name_len = u32::from_be_bytes(len_buf) as usize;

    // ファイル名の中身を読み取る
    let mut name_buf = vec![0u8; name_len];
    stream.read_exact(&mut name_buf)?;
    let filename = String::from_utf8_lossy(&name_buf).to_string();

    // ファイルサイズ(8バイト)を読み取る
    let mut size_buf = [0u8; 8];
    stream.read_exact(&mut size_buf)?;
    let size = u64::from_be_bytes(size_buf);

    // 送られてきたファイル名そのままだと元のファイルを上書きしかねないので、頭に "received_" を付けて保存する
    let save_path = PathBuf::from(format!("received_{filename}"));
    let mut out_file = fs::File::create(&save_path)?;

    // stream.take(size): 「このストリームから最大size バイトだけ読む」という制限付きの読み取り口を作る
    // （これが無いと「ファイルの終わり」がどこか分からず、ずっと読み続けようとしてしまう。
    // C++で自前にTCPの区切りを実装するときに、あらかじめ聞いておいたサイズ分だけ
    // read()し続けるループを書くのと同じ発想を、標準ライブラリの型で表現したものです）
    let mut limited = stream.take(size);
    io::copy(&mut limited, &mut out_file)?;

    Ok((filename, size, save_path))
}
