fn main() {
    // Windows：把 res/app.ico 嵌入 exe，作为文件/任务栏图标
    #[cfg(windows)]
    {
        let _ = embed_resource::compile("res/app.rc", embed_resource::NONE);
    }
}
