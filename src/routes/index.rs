use crate::{
    config::Config,
    data::{Node, State},
    error::{Error, ErrorKind},
};
use actix_web::{web, HttpResponse};
use rand::{seq::SliceRandom, thread_rng};
use std::io::BufWriter;

fn open_reg(node: &Node) -> bool {
    node.instance
        .as_ref()
        .map(|i| i.reg)
        .or_else(|| node.info.as_ref().map(|i| i.reg))
        .unwrap_or(false)
}

#[tracing::instrument(name = "Index", skip(config, state))]
pub(crate) async fn route(
    state: web::Data<State>,
    config: web::Data<Config>,
) -> Result<HttpResponse, Error> {
    let all_nodes = state.node_cache().nodes().await?;

    let mut nodes = Vec::new();
    let mut local = Vec::new();

    for node in all_nodes {
        if node
            .base
            .authority_str()
            .map(|authority| {
                config
                    .local_domains()
                    .iter()
                    .find(|domain| domain.as_str() == authority)
                    .is_some()
            })
            .unwrap_or(false)
        {
            local.push(node);
        } else {
            nodes.push(node);
        }
    }

    nodes.sort_by(|lhs, rhs| match (open_reg(lhs), open_reg(rhs)) {
        (true, true) | (false, false) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
    });

    if let Some((i, _)) = nodes.iter().enumerate().find(|(_, node)| !open_reg(node)) {
        nodes[..i].shuffle(&mut thread_rng());
        nodes[i..].shuffle(&mut thread_rng());
    } else {
        nodes.shuffle(&mut thread_rng());
    }

    let mut buf = BufWriter::new(Vec::new());

    crate::templates::index(&mut buf, &local, &nodes, &config)?;
    let buf = buf.into_inner().map_err(|e| {
        tracing::error!("Error rendering template, {}", e.error());
        ErrorKind::FlushBuffer
    })?;

    Ok(HttpResponse::Ok().content_type("text/html").body(buf))
}
