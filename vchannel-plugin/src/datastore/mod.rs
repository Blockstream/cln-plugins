pub mod entity;

use crate::datastore::entity::{Payment, VirtualChannel};
use anyhow::Context;
use anyhow::Result;
use cln_rpc::ClnRpc;
use cln_rpc::model::requests::{DatastoreMode, DatastoreRequest, ListdatastoreRequest};
use cln_rpc::model::responses::ListdatastoreResponse;

pub struct Datastore<'a> {
    cln_rpc: &'a mut ClnRpc,
}

impl<'a> Datastore<'a> {
    pub fn new(cln_rpc: &'a mut ClnRpc) -> Self {
        Self { cln_rpc }
    }

    pub async fn must_create_virtual_channel(&mut self, channel: &VirtualChannel) -> Result<()> {
        let _ = self
            .cln_rpc
            .call_typed(&DatastoreRequest {
                generation: None,
                hex: None,
                mode: Some(DatastoreMode::MUST_CREATE),
                string: Some(
                    serde_json::to_string(&channel)
                        .context("failed to serialize virtual channel")?,
                ),
                key: vec![
                    "vchannel".to_string(),
                    "channels".to_string(),
                    channel.id.clone(),
                ],
            })
            .await
            .context("failed to call datastore request")?;

        Ok(())
    }

    pub async fn must_update_virtual_channel(&mut self, channel: &VirtualChannel) -> Result<()> {
        let _ = self
            .cln_rpc
            .call_typed(&DatastoreRequest {
                generation: None,
                hex: None,
                mode: Some(DatastoreMode::MUST_REPLACE),
                string: Some(
                    serde_json::to_string(&channel)
                        .context("failed to serialize virtual channel")?,
                ),
                key: vec![
                    "vchannel".to_string(),
                    "channels".to_string(),
                    channel.id.clone(),
                ],
            })
            .await
            .context("failed to call datastore request")?;

        Ok(())
    }

    pub async fn get_virtual_channel(
        &mut self,
        virtual_channel_id: &String,
    ) -> Result<Option<VirtualChannel>> {
        let ds_res: ListdatastoreResponse = self
            .cln_rpc
            .call_typed(&ListdatastoreRequest {
                key: Some(vec![
                    "vchannel".to_string(),
                    "channels".to_string(),
                    virtual_channel_id.clone(),
                ]),
            })
            .await
            .context("failed to call listdatastore request")?;

        let Some(ds_entry) = ds_res.datastore.first() else {
            return Ok(None);
        };

        let Some(data) = ds_entry.string.clone() else {
            return Ok(None);
        };

        let mut channel: VirtualChannel =
            serde_json::from_str(&data).context("failed to deserialize virtual channel")?;

        channel.id = virtual_channel_id.clone();

        Ok(Some(channel))
    }

    pub async fn list_virtual_channels(&mut self) -> Result<Vec<VirtualChannel>> {
        let ds_res: ListdatastoreResponse = self
            .cln_rpc
            .call_typed(&ListdatastoreRequest {
                key: Some(vec!["vchannel".to_string(), "channels".to_string()]),
            })
            .await
            .context("failed to call listdatastore request")?;

        let mut channels: Vec<VirtualChannel> = Vec::with_capacity(ds_res.datastore.len());

        for ds_entry in ds_res.datastore {
            let Some(data) = ds_entry.string.clone() else {
                continue;
            };

            let mut channel: VirtualChannel =
                serde_json::from_str(&data).context("failed to deserialize virtual channel")?;

            // Should exist but if not let's keep empty
            if let Some(scid) = ds_entry.key.last() {
                channel.id = scid.clone();
            }

            channels.push(channel);
        }

        Ok(channels)
    }

    pub async fn must_create_payment(&mut self, payment: &Payment) -> Result<()> {
        let _ = self
            .cln_rpc
            .call_typed(&DatastoreRequest {
                generation: None,
                hex: None,
                mode: Some(DatastoreMode::MUST_CREATE),
                string: Some(
                    serde_json::to_string(&payment).context("failed to serialize payment")?,
                ),
                key: vec![
                    "vchannel".to_string(),
                    "payments".to_string(),
                    hex::encode(payment.payment_hash.clone()),
                ],
            })
            .await
            .context("failed to call datastore request")?;

        Ok(())
    }

    pub async fn must_update_payment(&mut self, payment: &Payment) -> Result<()> {
        let _ = self
            .cln_rpc
            .call_typed(&DatastoreRequest {
                generation: None,
                hex: None,
                mode: Some(DatastoreMode::MUST_REPLACE),
                string: Some(
                    serde_json::to_string(&payment).context("failed to serialize payment")?,
                ),
                key: vec![
                    "vchannel".to_string(),
                    "payments".to_string(),
                    hex::encode(payment.payment_hash.clone()),
                ],
            })
            .await
            .context("failed to call datastore request")?;
        Ok(())
    }

    pub async fn get_payment(&mut self, payment_hash: &Vec<u8>) -> Result<Option<Payment>> {
        let ds_res: ListdatastoreResponse = self
            .cln_rpc
            .call_typed(&ListdatastoreRequest {
                key: Some(vec![
                    "vchannel".to_string(),
                    "payments".to_string(),
                    hex::encode(payment_hash),
                ]),
            })
            .await
            .context("failed to call listdatastore request")?;

        let Some(ds_entry) = ds_res.datastore.first() else {
            return Ok(None);
        };

        let Some(data) = ds_entry.string.clone() else {
            return Ok(None);
        };

        let mut payment: Payment =
            serde_json::from_str(&data).context("failed to deserialize payment")?;

        payment.payment_hash = payment_hash.clone();
        Ok(Some(payment))
    }

    pub async fn is_payment_exist(&mut self, payment_hash: &Vec<u8>) -> Result<bool> {
        let ds_res: ListdatastoreResponse = self
            .cln_rpc
            .call_typed(&ListdatastoreRequest {
                key: Some(vec![
                    "vchannel".to_string(),
                    "payments".to_string(),
                    hex::encode(payment_hash),
                ]),
            })
            .await
            .context("failed to call listdatastore request")?;

        let Some(ds_entry) = ds_res.datastore.first() else {
            return Ok(false);
        };

        Ok(ds_entry.string.is_some())
    }
}
