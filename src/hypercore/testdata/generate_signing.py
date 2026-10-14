# Regenerate from the repository root:
# uv run --with hyperliquid-python-sdk==0.24.0 python src/hypercore/testdata/generate_signing.py
# The fixture uses only throwaway keys and performs no network/API transactions.
import json
from importlib.metadata import version
from pathlib import Path
from eth_account import Account
from eth_account.messages import encode_typed_data
from eth_utils import keccak
from hyperliquid.utils import signing as ref

nonce = 1700000000000
user = '0x1111111111111111111111111111111111111111'
key = Account.from_key('0x' + '00' * 31 + '01')
lead = key.address.lower()
other = '0x2222222222222222222222222222222222222222'
agent_types = [dict(name=n, type=t) for n,t in [('hyperliquidChain','string'),('agentAddress','address'),('agentName','string'),('nonce','uint64')]]
def encoded(data):
    signable = encode_typed_data(full_message=data)
    return '0x' + keccak(b'\x19' + signable.version + signable.header + signable.body).hex()

cases = []
contexts = []
for chain,chain_id in [('Mainnet',42161),('Testnet',421614)]:
    base = dict(signatureChainId=hex(chain_id), hyperliquidChain=chain)
    actions = [
      ('ApproveAgent', dict(type='approveAgent', **base, agentAddress=other, agentName=name, nonce=nonce), agent_types)
      for name in ['', 'bot']
    ] + [
      ('TokenDelegate', dict(type='tokenDelegate', **base, validator=other, wei=100000000, isUndelegate=undelegate, nonce=nonce), ref.TOKEN_DELEGATE_TYPES)
      for undelegate in [False, True]
    ]
    for kind, action, types in actions:
      normal = ref.user_signed_payload('HyperliquidTransaction:'+kind, types, action)
      multi = ref.user_signed_payload('HyperliquidTransaction:'+kind, ref.add_multi_sig_types(types), ref.add_multi_sig_fields(action,user,lead))
      cases.append(dict(chain=chain,nonce=nonce,action=action,prehash=encoded(normal),multisigPrehash=encoded(multi)))
    for vault in [None, other]:
      for expiry in [None, nonce+10000]:
        action = dict(type='borrowLend',operation='supply',token=0,amount='10')
        connection = ref.action_hash([user,lead,action],vault,nonce,expiry)
        data = ref.l1_payload(ref.construct_phantom_agent(connection,chain=='Mainnet'))
        contexts.append(dict(chain=chain,nonce=nonce,action=action,vault=vault,expiresAfter=expiry,prehash=encoded(data)))
# The Python SDK's own signing helpers always sign with signatureChainId 0x66eee,
# including on Mainnet, so these cases pin a chain ID that differs from the network's.
python_default = []
signers = dict(authorizedUsers=sorted([other, lead, user]), threshold=2)
for chain in ['Mainnet', 'Testnet']:
    is_mainnet = chain == 'Mainnet'
    convert = dict(type='convertToMultiSigUser', signers=json.dumps(signers), nonce=nonce)
    convert_sig = ref.sign_convert_to_multi_sig_user_action(key, convert, is_mainnet)
    inner = dict(type='usdSend', signatureChainId='0x66eee', hyperliquidChain=chain, destination=other, amount='12.50', time=nonce)
    participant = ref.sign_multi_sig_user_signed_action_payload(key, inner, is_mainnet, ref.USD_SEND_SIGN_TYPES, 'HyperliquidTransaction:UsdSend', user, lead)
    multi = dict(type='multiSig', signatureChainId='0x66eee', signatures=[participant], payload=dict(multiSigUser=user, outerSigner=lead, action=inner))
    outer = ref.sign_multi_sig_action(key, multi, is_mainnet, None, nonce, None)
    python_default.append(dict(chain=chain, nonce=nonce, convert=convert, convertSignature=convert_sig, multiSig=multi, outerSignature=outer))
fixture = dict(source='hyperliquid-python-sdk '+version('hyperliquid-python-sdk')+' hyperliquid/utils/signing.py',multisigUser=user,leader=lead,userSigned=cases,l1Contexts=contexts,pythonDefaultChainId=python_default)
Path('src/hypercore/testdata/signing.json').write_text(json.dumps(fixture,indent=2)+'\n')
print(fixture['source'], ':', len(cases), 'user-signed and',len(contexts),'L1 fixtures')
