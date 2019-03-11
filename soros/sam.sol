pragma solidity >=0.4.17;

contract Lottery {
    address public manager;
    address payable [] public players;

    constructor () public {
        manager = msg.sender;
    }

    function enter() public payable {
        require(msg.value > 0.01 ether);
        players.push(msg.sender);
    }

    function random() private view returns (uint) {
        return uint256(keccak256(abi.encodePacked(block.difficulty, now, players)));
    }

    function pickWinner() public restricted {
        require(msg.sender == manager);
        uint256 index = random() % players.length;
        players[index].transfer(address(this).balance);
        players.length = 0;
    }

    modifier restricted() {
        require(msg.sender == manager);
        _;
    }

}