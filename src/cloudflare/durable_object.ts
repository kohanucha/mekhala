export function getDurableStub(
  env: Record<string, unknown>,
  region?: string,
): DurableObjectStub {
  const ns = env.MEKHALA_NWC_DO as unknown as DurableObjectNamespace;
  if (region && region !== '') {
    return ns.getByName('GLOBAL', { locationHint: region as DurableObjectLocationHint });
  }
  const id = ns.idFromName('GLOBAL');
  return ns.get(id);
}
