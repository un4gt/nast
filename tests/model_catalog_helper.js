// The model catalog is independent from sampling/prompt settings.
module.exports = async function configureMockModel(call, endpoint, upstreamModel, key) {
  const catalog = await call('model_catalog.get', {});
  const route = catalog.models[0].routes[0];
  route.config.endpoint = endpoint;
  route.config.context_limit = null;
  route.config.output_limit = null;
  route.upstream_model = upstreamModel;
  await call('model_catalog.save', { catalog, ...(key === undefined ? {} : { credentials: { [route.id]: key } }) });
};
