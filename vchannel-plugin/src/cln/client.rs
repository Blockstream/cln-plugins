use anyhow::{Context, Result};
use cln_rpc::ClnRpc;
use cln_rpc::model::requests::{
    InjectpaymentonionRequest, SendcustommsgRequest, WaitblockheightRequest,
};
use cln_rpc::model::responses::{
    InjectpaymentonionResponse, SendcustommsgResponse, WaitblockheightResponse,
};

pub struct ClnClient<'a> {
    cln_rpc: &'a mut ClnRpc,
}

impl<'a> ClnClient<'a> {
    pub fn new(cln_rpc: &'a mut ClnRpc) -> Self {
        ClnClient { cln_rpc }
    }

    pub async fn call_injectpaymentonion(
        &mut self,
        req: &InjectpaymentonionRequest,
    ) -> Result<InjectpaymentonionResponse> {
        self.cln_rpc
            .call_typed(req)
            .await
            .context("failed to call injectpaymentonion request")
    }

    pub async fn call_sendcustommsg(
        &mut self,
        req: &SendcustommsgRequest,
    ) -> Result<SendcustommsgResponse> {
        self.cln_rpc
            .call_typed(req)
            .await
            .context("failed to call sendcustommsg")
    }

    pub async fn call_waitblockheight(
        &mut self,
        req: &WaitblockheightRequest,
    ) -> Result<WaitblockheightResponse> {
        self.cln_rpc
            .call_typed(req)
            .await
            .context("failed to call waitblockheight request")
    }
}
