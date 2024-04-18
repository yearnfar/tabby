use std::{fs, io::IsTerminal};

use anyhow::Result;
use kdam::BarExt;
use tabby_common::{
    config::RepositoryConfig,
    index::{register_tokenizers, CodeSearchSchema},
    path::index_dir,
    SourceFile,
};
use tantivy::{directory::MmapDirectory, doc, Index};
use tracing::warn;

use crate::{code::CodeIntelligence, utils::tqdm};

// Magic numbers
static MAX_LINE_LENGTH_THRESHOLD: usize = 300;
static AVG_LINE_LENGTH_THRESHOLD: f32 = 150f32;

pub fn index_repositories(_config: &[RepositoryConfig]) -> Result<()> {
    let code = CodeSearchSchema::new();

    fs::create_dir_all(index_dir())?;
    let directory = MmapDirectory::open(index_dir())?;
    let index = Index::open_or_create(directory, code.schema)?;
    register_tokenizers(&index);

    // Initialize the search index writer with an initial arena size of 150 MB.
    let mut writer = index.writer(150_000_000)?;
    writer.delete_all_documents()?;

    let total_file_size: usize = SourceFile::all()?
        .filter(is_valid_file)
        .map(|x| x.read_file_size())
        .sum();

    let mut pb = std::io::stdout()
        .is_terminal()
        .then(|| tqdm(total_file_size));

    let intelligence = CodeIntelligence::default();
    for file in SourceFile::all()?.filter(is_valid_file) {
        let text = match file.read_content() {
            Ok(content) => content,
            Err(e) => {
                warn!("Failed to read content of '{}': {}", file.filepath, e);
                continue;
            }
        };

        for body in intelligence.chunks(&text) {
            pb.as_mut().map(|b| b.update(body.len())).transpose()?;

            writer.add_document(doc!(
                    code.field_git_url => file.git_url.clone(),
                    code.field_filepath => file.filepath.clone(),
                    code.field_language => file.language.clone(),
                    code.field_body => body,
            ))?;
        }
    }

    writer.commit()?;
    writer.wait_merging_threads()?;

    Ok(())
}

fn is_valid_file(file: &SourceFile) -> bool {
    file.max_line_length <= MAX_LINE_LENGTH_THRESHOLD
        && file.avg_line_length <= AVG_LINE_LENGTH_THRESHOLD
}
