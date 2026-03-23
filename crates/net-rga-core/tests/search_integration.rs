mod common;

use std::fs;

use net_rga_core::{
    ConfigStore, CorpusConfig, CorpusId, ProviderConfig, RuntimePaths, S3ConnectionConfig,
    S3Provider, SearchOutputFormat, SearchRequest, execute_search, filter_manifest_documents,
    rank_documents, sync_corpus_with_provider,
};
use net_rga_core::search_engine::execute_search_with_provider;
use tokio::runtime::Runtime;

use common::{
    MINIO_REGION, MinioHarness, build_minio_client, temp_dir, wait_for_minio,
};

#[test]
fn local_search_finds_pdf_content_after_sync() {
    let state_root = temp_dir("net-rga-local-search");
    let corpus_root = temp_dir("net-rga-local-search-corpus");
    let pdf_path = corpus_root.join("docs/report.pdf");
    if let Some(parent) = pdf_path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|error| panic!("pdf dir should create: {error}"));
    }
    fs::write(&pdf_path, build_test_pdf(&["Riverglass PDF page one", "Second page note"]))
        .unwrap_or_else(|error| panic!("pdf fixture should write: {error}"));

    let paths = RuntimePaths::from_state_root(state_root.clone());
    let store = ConfigStore::new(paths.clone());
    store
        .add_corpus(CorpusConfig {
            id: "local".to_owned(),
            display_name: Some("Local".to_owned()),
            provider: ProviderConfig::LocalFs {
                root: corpus_root.clone(),
            },
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            backend: None,
        })
        .unwrap_or_else(|error| panic!("corpus should save: {error}"));

    net_rga_core::sync_corpus(&paths, "local")
        .unwrap_or_else(|error| panic!("sync should succeed: {error}"));

    let response = execute_search(
        &paths,
        &SearchRequest {
            corpus_id: CorpusId("local".to_owned()),
            query: "Riverglass PDF page one".to_owned(),
            fixed_strings: true,
            path_globs: Vec::new(),
            extensions: Vec::new(),
            content_types: Vec::new(),
            size_min: None,
            size_max: None,
            modified_after: None,
            modified_before: None,
            limit: None,
            output_format: SearchOutputFormat::Json,
        },
    )
    .unwrap_or_else(|error| panic!("search should succeed: {error}"));

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].anchor.locator.page, Some(1));

    fs::remove_dir_all(state_root).ok();
    fs::remove_dir_all(corpus_root).ok();
}

#[test]
fn s3_search_finds_synced_objects_against_minio() {
    let Some(harness) = MinioHarness::start() else {
        eprintln!("skipping s3 search integration test because docker is unavailable");
        return;
    };

    let runtime = Runtime::new().unwrap_or_else(|error| panic!("tokio runtime should build: {error}"));
    let client = build_minio_client(&runtime, &harness.endpoint);
    wait_for_minio(&runtime, &client);

    runtime.block_on(async {
        client
            .create_bucket()
            .bucket(&harness.bucket)
            .send()
            .await
            .unwrap_or_else(|error| panic!("bucket should create: {error}"));
        client
            .put_object()
            .bucket(&harness.bucket)
            .key("docs/report.txt")
            .body("riverglass lives in minio".as_bytes().to_vec().into())
            .send()
            .await
            .unwrap_or_else(|error| panic!("object should upload: {error}"));
    });

    let state_root = temp_dir("net-rga-s3-search");
    let paths = RuntimePaths::from_state_root(state_root.clone());
    let corpus = CorpusConfig {
        id: "s3".to_owned(),
        display_name: Some("S3".to_owned()),
        provider: ProviderConfig::S3 {
            bucket: harness.bucket.clone(),
            prefix: Some("docs".to_owned()),
            region: Some(MINIO_REGION.to_owned()),
            endpoint: Some(harness.endpoint.clone()),
            profile: None,
        },
        include_globs: Vec::new(),
        exclude_globs: Vec::new(),
        backend: None,
    };
    let store = ConfigStore::new(paths.clone());
    store
        .add_corpus(corpus.clone())
        .unwrap_or_else(|error| panic!("corpus should save: {error}"));

    let provider = S3Provider::from_parts(
        S3ConnectionConfig::from_provider_config(&corpus.provider)
            .unwrap_or_else(|error| panic!("s3 config should parse: {error}")),
        runtime,
        client,
    );

    sync_corpus_with_provider(&paths, &corpus, &provider)
        .unwrap_or_else(|error| panic!("sync should succeed: {error}"));

    let request = SearchRequest {
        corpus_id: CorpusId("s3".to_owned()),
        query: "riverglass lives in minio".to_owned(),
        fixed_strings: true,
        path_globs: Vec::new(),
        extensions: Vec::new(),
        content_types: Vec::new(),
        size_min: None,
        size_max: None,
        modified_after: None,
        modified_before: None,
        limit: None,
        output_format: SearchOutputFormat::Json,
    };
    let response = execute_search_with_provider(
        &request,
        &corpus,
        &provider,
        rank_documents(
            filter_manifest_documents(&paths, &request)
                .unwrap_or_else(|error| panic!("manifest candidates should load: {error}")),
            &request,
        ),
    )
    .unwrap_or_else(|error| panic!("search should succeed: {error}"));

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].document_id.0, "report.txt");

    fs::remove_dir_all(state_root).ok();
}

fn build_test_pdf(pages: &[&str]) -> Vec<u8> {
    let mut objects = Vec::new();
    let mut page_ids = Vec::new();
    let font_id = u32::try_from(3 + (pages.len() * 2)).unwrap_or(u32::MAX);

    objects.push((1_u32, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()));

    for (index, page_text) in pages.iter().enumerate() {
        let page_id = u32::try_from(3 + (index * 2)).unwrap_or(u32::MAX);
        let content_id = page_id + 1;
        page_ids.push(format!("{page_id} 0 R"));

        let page_object = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {content_id} 0 R /Resources << /Font << /F1 {font_id} 0 R >> >> >>"
        );
        objects.push((page_id, page_object));

        let stream = format!(
            "BT\n/F1 18 Tf\n72 720 Td\n({}) Tj\nET",
            escape_pdf_text(page_text)
        );
        let content_object = format!(
            "<< /Length {} >>\nstream\n{stream}\nendstream",
            stream.len()
        );
        objects.push((content_id, content_object));
    }

    objects.insert(
        1,
        (
            2_u32,
            format!("<< /Type /Pages /Kids [{}] /Count {} >>", page_ids.join(" "), pages.len()),
        ),
    );
    objects.push((font_id, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned()));

    let mut buffer = Vec::new();
    buffer.extend_from_slice(b"%PDF-1.4\n");
    let mut offsets = vec![0_usize];
    for (id, body) in &objects {
        offsets.push(buffer.len());
        buffer.extend_from_slice(format!("{id} 0 obj\n{body}\nendobj\n").as_bytes());
    }

    let xref_offset = buffer.len();
    buffer.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    buffer.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        buffer.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    buffer.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    buffer
}

fn escape_pdf_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}
